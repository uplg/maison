use std::{
    collections::{HashMap, HashSet},
    env,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use axum::http::StatusCode;
use rumqttc::{AsyncClient, ClientError, ConnectionError, Event, Incoming, MqttOptions, QoS};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    sync::RwLock,
    task::JoinHandle,
    time::{Duration, sleep},
};
use tracing::{debug, info, warn};

use crate::{
    config::Config,
    error::AppError,
    zigbee_native::{NativeKnownDevice, NativeZigbeeCommand, NativeZigbeeRuntime, ZigbeeDeviceType},
};

const MQTT_RECONNECT_DELAY: Duration = Duration::from_secs(5);
const MQTT_KEEP_ALIVE: Duration = Duration::from_secs(30);
const MQTT_MAX_PACKET_SIZE: usize = 1024 * 1024;
const MQTT_REQUEST_CAPACITY: usize = 100;

#[derive(Debug, Clone, Serialize)]
pub struct ZigbeeLampState {
    #[serde(rename = "isOn")]
    pub is_on: bool,
    pub brightness: u8,
    pub temperature: Option<u8>,
    #[serde(rename = "temperatureMin")]
    pub temperature_min: Option<u8>,
    #[serde(rename = "temperatureMax")]
    pub temperature_max: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZigbeeLampView {
    pub id: String,
    pub name: String,
    pub address: String,
    #[serde(rename = "friendlyName")]
    pub friendly_name: String,
    #[serde(rename = "linkQuality")]
    pub link_quality: Option<u16>,
    #[serde(rename = "interviewCompleted")]
    pub interview_completed: bool,
    pub model: Option<String>,
    pub manufacturer: String,
    pub firmware: Option<String>,
    pub connected: bool,
    pub reachable: bool,
    #[serde(rename = "supportsBrightness")]
    pub supports_brightness: bool,
    #[serde(rename = "supportsTemperature")]
    pub supports_temperature: bool,
    pub state: ZigbeeLampState,
    #[serde(rename = "lastSeen")]
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZigbeeStats {
    pub total: usize,
    pub connected: usize,
    pub reachable: usize,
    pub disabled: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZigbeePairingStatus {
    pub active: bool,
    #[serde(rename = "remainingSeconds")]
    pub remaining_seconds: u16,
    #[serde(rename = "permitJoinSeconds")]
    pub permit_join_seconds: u16,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredZigbeeLampConfig {
    id: String,
    name: String,
    friendly_name: String,
    ieee_address: String,
    node_id: Option<u16>,
    endpoint: Option<u8>,
    #[serde(default)]
    input_clusters: Vec<u16>,
    #[serde(default)]
    output_clusters: Vec<u16>,
    model: Option<String>,
    manufacturer: Option<String>,
    firmware: Option<String>,
    supports_brightness: bool,
    supports_temperature: bool,
    color_temp_min: Option<u16>,
    color_temp_max: Option<u16>,
    #[serde(default)]
    is_remote: bool,
}

#[derive(Clone)]
pub struct ZigbeeManager {
    inner: Arc<ZigbeeManagerInner>,
    native: Option<NativeZigbeeManager>,
}

struct ZigbeeManagerInner {
    store: ZigbeeStore,
    lamps: RwLock<HashMap<String, ZigbeeLampRuntime>>,
    pending_states: RwLock<HashMap<String, Value>>,
    blacklisted_addresses: RwLock<HashSet<String>>,
    availability_message: RwLock<Option<String>>,
    pairing: RwLock<PairingRuntime>,
    mqtt_client: RwLock<Option<AsyncClient>>,
    mqtt_host: String,
    mqtt_port: u16,
    mqtt_username: Option<String>,
    mqtt_password: Option<String>,
    mqtt_client_id: String,
    base_topic: String,
    permit_join_seconds: u16,
    shutting_down: AtomicBool,
    mqtt_task: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone)]
struct ZigbeeLampRuntime {
    config: StoredZigbeeLampConfig,
    state: RuntimeLampState,
    connected: bool,
    reachable: bool,
    link_quality: Option<u16>,
    last_seen: Option<String>,
    interview_completed: bool,
}

#[derive(Clone)]
struct RuntimeLampState {
    is_on: bool,
    brightness: u8,
    temperature: Option<u8>,
    temperature_min: Option<u8>,
    temperature_max: Option<u8>,
}

#[derive(Default)]
struct PairingRuntime {
    active: bool,
    deadline: Option<Instant>,
    message: Option<String>,
}

#[derive(Clone)]
struct ZigbeeStore {
    lamps_path: PathBuf,
    blacklist_path: PathBuf,
}

impl ZigbeeManager {
    pub fn new(config: &Config) -> Result<Self, AppError> {
        let backend = env::var("ZIGBEE_BACKEND")
            .unwrap_or_else(|_| "mqtt".to_string())
            .to_ascii_lowercase();
        let store = ZigbeeStore {
            lamps_path: config.zigbee_lamps_path.clone(),
            blacklist_path: config.zigbee_lamps_blacklist_path.clone(),
        };

        let blacklisted_addresses = store.load_blacklist();
        let lamps: HashMap<String, ZigbeeLampRuntime> = store
            .load_lamps()?
            .into_iter()
            .map(|lamp| {
                let state = RuntimeLampState {
                    is_on: false,
                    brightness: 0,
                    temperature: None,
                    temperature_min: lamp.color_temp_min.map(|_| 0),
                    temperature_max: lamp.color_temp_max.map(|_| 100),
                };

                (
                    lamp.id.clone(),
                    ZigbeeLampRuntime {
                        config: lamp,
                        state,
                        connected: false,
                        reachable: false,
                        link_quality: None,
                        last_seen: None,
                        interview_completed: true,
                    },
                )
            })
            .collect();

        let manager = Self {
            inner: Arc::new(ZigbeeManagerInner {
                store,
                lamps: RwLock::new(lamps),
                pending_states: RwLock::new(HashMap::new()),
                blacklisted_addresses: RwLock::new(blacklisted_addresses),
                availability_message: RwLock::new(Some("Connecting to Zigbee2MQTT".to_string())),
                pairing: RwLock::new(PairingRuntime::default()),
                mqtt_client: RwLock::new(None),
                mqtt_host: config.mqtt_host.clone(),
                mqtt_port: config.mqtt_port,
                mqtt_username: config.mqtt_username.clone(),
                mqtt_password: config.mqtt_password.clone(),
                mqtt_client_id: config.mqtt_client_id.clone(),
                base_topic: config.z2m_base_topic.clone(),
                permit_join_seconds: config.zigbee_permit_join_seconds,
                shutting_down: AtomicBool::new(false),
                mqtt_task: Mutex::new(None),
            }),
            native: if backend == "native" || backend == "native_stub" {
                Some(NativeZigbeeManager::new(config)?)
            } else {
                None
            },
        };

        if manager.native.is_none() {
            manager.spawn_mqtt_task();
        }
        Ok(manager)
    }

    pub async fn list_lamps(&self) -> Vec<ZigbeeLampView> {
        if let Some(native) = &self.native {
            return native.list_lamps().await;
        }
        let lamps = self.inner.lamps.read().await;
        let mut values = lamps.values().map(to_view).collect::<Vec<_>>();
        values.sort_by(|left, right| left.name.cmp(&right.name));
        values
    }

    pub async fn get_lamp(&self, lamp_id: &str) -> Option<ZigbeeLampView> {
        if let Some(native) = &self.native {
            return native.get_lamp(lamp_id).await;
        }
        let lamps = self.inner.lamps.read().await;
        lamps.get(lamp_id).map(to_view)
    }

    pub async fn stats(&self) -> ZigbeeStats {
        if let Some(native) = &self.native {
            return native.stats().await;
        }
        let lamps = self.inner.lamps.read().await;
        let total = lamps.len();
        let connected = lamps.values().filter(|lamp| lamp.connected).count();
        let reachable = lamps.values().filter(|lamp| lamp.reachable).count();
        let message = self.inner.availability_message.read().await.clone();

        ZigbeeStats {
            total,
            connected,
            reachable,
            disabled: false,
            message,
        }
    }

    pub async fn pairing_status(&self) -> ZigbeePairingStatus {
        if let Some(native) = &self.native {
            return native.pairing_status().await;
        }
        let mut pairing = self.inner.pairing.write().await;
        let remaining_seconds = remaining_seconds(&mut pairing);

        ZigbeePairingStatus {
            active: pairing.active,
            remaining_seconds,
            permit_join_seconds: self.inner.permit_join_seconds,
            message: pairing.message.clone(),
        }
    }

    pub async fn start_pairing(&self) -> Result<ZigbeePairingStatus, AppError> {
        if let Some(native) = &self.native {
            return native.start_pairing().await;
        }
        let seconds = self.inner.permit_join_seconds;
        self.publish_bridge_request("permit_join", json!({
            "time": seconds,
        }))
        .await?;

        let mut pairing = self.inner.pairing.write().await;
        pairing.active = true;
        pairing.deadline = Some(Instant::now() + Duration::from_secs(u64::from(seconds)));
        pairing.message = Some("Pairing window is open".to_string());
        let remaining_seconds = remaining_seconds(&mut pairing);

        Ok(ZigbeePairingStatus {
            active: pairing.active,
            remaining_seconds,
            permit_join_seconds: seconds,
            message: pairing.message.clone(),
        })
    }

    pub async fn stop_pairing(&self) -> Result<ZigbeePairingStatus, AppError> {
        if let Some(native) = &self.native {
            return native.stop_pairing().await;
        }
        self.publish_bridge_request("permit_join", json!({
            "time": 0,
        }))
        .await?;

        let mut pairing = self.inner.pairing.write().await;
        pairing.active = false;
        pairing.deadline = None;
        pairing.message = Some("Pairing window is closed".to_string());

        Ok(ZigbeePairingStatus {
            active: false,
            remaining_seconds: 0,
            permit_join_seconds: self.inner.permit_join_seconds,
            message: pairing.message.clone(),
        })
    }

    pub async fn set_power(&self, lamp_id: &str, enabled: bool) -> Result<ZigbeeLampState, AppError> {
        if let Some(native) = &self.native {
            return native.set_power(lamp_id, enabled).await;
        }
        let friendly_name = self.friendly_name_for(lamp_id).await?;
        self.publish_device_set(&friendly_name, json!({
            "state": if enabled { "ON" } else { "OFF" },
        }))
        .await?;

        self.update_state_after_command(lamp_id, |lamp| {
            lamp.state.is_on = enabled;
            lamp.connected = true;
            lamp.reachable = true;
        })
        .await
    }

    pub async fn set_brightness(
        &self,
        lamp_id: &str,
        brightness: u8,
    ) -> Result<ZigbeeLampState, AppError> {
        if let Some(native) = &self.native {
            return native.set_brightness(lamp_id, brightness).await;
        }
        let friendly_name = self.friendly_name_for(lamp_id).await?;
        self.publish_device_set(&friendly_name, json!({
            "brightness": to_brightness(brightness),
        }))
        .await?;

        self.update_state_after_command(lamp_id, |lamp| {
            lamp.state.brightness = brightness.clamp(0, 100);
            if brightness > 0 {
                lamp.state.is_on = true;
            }
            lamp.connected = true;
            lamp.reachable = true;
        })
        .await
    }

    pub async fn set_temperature(
        &self,
        lamp_id: &str,
        temperature: u8,
    ) -> Result<ZigbeeLampState, AppError> {
        if let Some(native) = &self.native {
            return native.set_temperature(lamp_id, temperature).await;
        }
        let friendly_name = self.friendly_name_for(lamp_id).await?;
        let (min, max) = {
            let lamps = self.inner.lamps.read().await;
            let lamp = lamps.get(lamp_id).ok_or_else(|| not_found("Zigbee lamp not found"))?;
            if !lamp.config.supports_temperature {
                return Err(AppError::http(
                    StatusCode::BAD_REQUEST,
                    "This Zigbee lamp does not support color temperature",
                ));
            }
            (
                lamp.config.color_temp_min.unwrap_or(153),
                lamp.config.color_temp_max.unwrap_or(500),
            )
        };

        self.publish_device_set(&friendly_name, json!({
            "color_temp": to_temperature(temperature, min, max),
        }))
        .await?;

        self.update_state_after_command(lamp_id, |lamp| {
            lamp.state.temperature = Some(temperature.clamp(0, 100));
            lamp.connected = true;
            lamp.reachable = true;
        })
        .await
    }

    pub async fn rename_lamp(&self, lamp_id: &str, name: &str) -> Result<(), AppError> {
        if let Some(native) = &self.native {
            return native.rename_lamp(lamp_id, name).await;
        }
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AppError::http(
                StatusCode::BAD_REQUEST,
                "Lamp name cannot be empty",
            ));
        }

        let stored = {
            let mut lamps = self.inner.lamps.write().await;
            let lamp = lamps
                .get_mut(lamp_id)
                .ok_or_else(|| not_found("Zigbee lamp not found"))?;
            lamp.config.name = trimmed.to_string();
            lamps.values().map(|lamp| lamp.config.clone()).collect::<Vec<_>>()
        };

        self.inner.store.save_lamps(&stored)
    }

    pub async fn shutdown(&self) {
        if let Some(native) = &self.native {
            native.shutdown().await;
            return;
        }
        self.inner.shutting_down.store(true, Ordering::SeqCst);
        *self.inner.mqtt_client.write().await = None;
        if let Some(handle) = self.inner.mqtt_task.lock().expect("mqtt task mutex").take() {
            handle.abort();
        }
    }

    fn spawn_mqtt_task(&self) {
        let manager = self.clone();
        let handle = tokio::spawn(async move {
            info!(
                host = %manager.inner.mqtt_host,
                port = manager.inner.mqtt_port,
                base_topic = %manager.inner.base_topic,
                "starting zigbee mqtt task"
            );
            manager.run_mqtt_loop().await;
        });
        *self.inner.mqtt_task.lock().expect("mqtt task mutex") = Some(handle);
    }

    async fn run_mqtt_loop(self) {
        while !self.inner.shutting_down.load(Ordering::SeqCst) {
            if let Err(error) = self.run_single_mqtt_session().await {
                warn!(error = %error, "zigbee mqtt session ended");
                self.set_availability_message(Some(format!(
                    "Zigbee2MQTT unavailable on {}:{}",
                    self.inner.mqtt_host, self.inner.mqtt_port
                )))
                .await;
                *self.inner.mqtt_client.write().await = None;
            }

            if self.inner.shutting_down.load(Ordering::SeqCst) {
                break;
            }

            sleep(MQTT_RECONNECT_DELAY).await;
        }
    }

    async fn run_single_mqtt_session(&self) -> Result<(), String> {
        let mut options = MqttOptions::new(
            self.inner.mqtt_client_id.clone(),
            self.inner.mqtt_host.clone(),
            self.inner.mqtt_port,
        );
        options.set_keep_alive(MQTT_KEEP_ALIVE);
        options.set_max_packet_size(MQTT_MAX_PACKET_SIZE, MQTT_MAX_PACKET_SIZE);

        if let Some(username) = self.inner.mqtt_username.as_deref() {
            options.set_credentials(username, self.inner.mqtt_password.as_deref().unwrap_or(""));
        }

        let (client, mut event_loop) = AsyncClient::new(options, MQTT_REQUEST_CAPACITY);
        info!("creating mqtt client for zigbee");
        *self.inner.mqtt_client.write().await = Some(client.clone());
        self.set_availability_message(Some("Waiting for Zigbee2MQTT bridge".to_string()))
            .await;
        info!("zigbee mqtt session created, waiting for connection");

        let mut initialized = false;

        loop {
            if self.inner.shutting_down.load(Ordering::SeqCst) {
                *self.inner.mqtt_client.write().await = None;
                return Ok(());
            }

            match event_loop.poll().await {
                Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                    info!("connected to mqtt broker for zigbee");
                    if !initialized {
                        self.subscribe_topics(&client)
                            .await
                            .map_err(client_error_to_string)?;
                        info!("subscribed to zigbee mqtt topics");
                        self.request_devices_refresh().await.ok();
                        initialized = true;
                    }
                }
                Ok(Event::Incoming(Incoming::Publish(publish))) => {
                    self.handle_publish(&publish.topic, &publish.payload).await;
                }
                Ok(_) => {}
                Err(error) => {
                    *self.inner.mqtt_client.write().await = None;
                    return Err(connection_error_to_string(error));
                }
            }
        }
    }

    async fn subscribe_topics(&self, client: &AsyncClient) -> Result<(), ClientError> {
        let base = self.inner.base_topic.trim_end_matches('/');
        client
            .subscribe(format!("{base}/bridge/state"), QoS::AtMostOnce)
            .await?;
        client
            .subscribe(format!("{base}/bridge/devices"), QoS::AtMostOnce)
            .await?;
        client
            .subscribe(format!("{base}/bridge/event"), QoS::AtMostOnce)
            .await?;
        client
            .subscribe(format!("{base}/bridge/response/devices"), QoS::AtMostOnce)
            .await?;
        client
            .subscribe(format!("{base}/+/availability"), QoS::AtMostOnce)
            .await?;
        client
            .subscribe(format!("{base}/+"), QoS::AtMostOnce)
            .await?;
        Ok(())
    }

    async fn handle_publish(&self, topic: &str, payload: &[u8]) {
        let base = self.inner.base_topic.trim_end_matches('/');
        let bridge_state_topic = format!("{base}/bridge/state");
        let bridge_devices_topic = format!("{base}/bridge/devices");
        let bridge_devices_response_topic = format!("{base}/bridge/response/devices");
        let bridge_event_topic = format!("{base}/bridge/event");

        if topic == bridge_state_topic {
            self.handle_bridge_state(payload).await;
            return;
        }

        if topic == bridge_devices_topic || topic == bridge_devices_response_topic {
            self.handle_bridge_devices(payload).await;
            self.request_states_refresh().await.ok();
            return;
        }

        if topic == bridge_event_topic {
            self.handle_bridge_event(payload).await;
            return;
        }

        if let Some(friendly_name) = topic.strip_prefix(&format!("{base}/")) {
            if let Some(name) = friendly_name.strip_suffix("/availability") {
                self.handle_availability(name, payload).await;
                return;
            }

            if !friendly_name.contains('/') {
                self.handle_device_state(friendly_name, payload).await;
            }
        }
    }

    async fn handle_bridge_state(&self, payload: &[u8]) {
        let state = serde_json::from_slice::<Value>(payload)
            .ok()
            .and_then(|value| {
                value
                    .get("state")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| String::from_utf8_lossy(payload).trim().to_string())
            .to_ascii_lowercase();
        if state == "online" {
            {
                let mut lamps = self.inner.lamps.write().await;
                for lamp in lamps.values_mut() {
                    if lamp.interview_completed {
                        lamp.connected = true;
                        lamp.reachable = true;
                    }
                }
            }
            self.set_availability_message(None).await;
            self.request_states_refresh().await.ok();
        } else {
            self.set_availability_message(Some(format!("Zigbee2MQTT bridge is {state}")))
                .await;
        }
    }

    async fn handle_bridge_event(&self, payload: &[u8]) {
        debug!(payload = %String::from_utf8_lossy(payload), "zigbee bridge event received");

        let Ok(value) = serde_json::from_slice::<Value>(payload) else {
            return;
        };

        let event_type = value.get("type").and_then(Value::as_str).unwrap_or_default();
        let mut pairing = self.inner.pairing.write().await;

        match event_type {
            "device_joined" => {
                pairing.message = Some("A Zigbee device joined the network".to_string());
            }
            "device_interview" => {
                let status = value
                    .pointer("/data/status")
                    .and_then(Value::as_str)
                    .unwrap_or("in_progress");
                pairing.message = Some(format!("Device interview {status}"));
                if status == "successful" {
                    drop(pairing);
                    self.request_devices_refresh().await.ok();
                    self.request_states_refresh().await.ok();
                    return;
                }
            }
            "device_announce" => {
                pairing.message = Some("A Zigbee device announced itself".to_string());
            }
            _ => {}
        }

        let _ = remaining_seconds(&mut pairing);
    }

    async fn handle_bridge_devices(&self, payload: &[u8]) {
        let Ok(devices) = serde_json::from_slice::<Value>(payload) else {
            warn!("failed to parse zigbee bridge devices payload");
            return;
        };

        let Some(entries) = bridge_devices_entries(&devices) else {
            warn!("zigbee bridge devices payload is not an array");
            return;
        };

        let blacklisted_addresses = self.inner.blacklisted_addresses.read().await.clone();
        let mut lamps = self.inner.lamps.write().await;
        let mut seen = HashSet::new();

        for entry in entries {
            let Some(discovered) = DiscoveredLamp::from_bridge_device(entry) else {
                continue;
            };

            if blacklisted_addresses.contains(&discovered.ieee_address) {
                continue;
            }

            let runtime = lamps.entry(discovered.id.clone()).or_insert_with(|| ZigbeeLampRuntime {
                config: StoredZigbeeLampConfig {
                    id: discovered.id.clone(),
                    name: discovered.name.clone(),
                    friendly_name: discovered.friendly_name.clone(),
                    ieee_address: discovered.ieee_address.clone(),
                    node_id: None,
                    endpoint: None,
                    input_clusters: Vec::new(),
                    output_clusters: Vec::new(),
                    model: discovered.model.clone(),
                    manufacturer: discovered.manufacturer.clone(),
                    firmware: discovered.firmware.clone(),
                    supports_brightness: discovered.supports_brightness,
                    supports_temperature: discovered.supports_temperature,
                    color_temp_min: discovered.color_temp_min,
                    color_temp_max: discovered.color_temp_max,
                    is_remote: false,
                },
                state: RuntimeLampState {
                    is_on: false,
                    brightness: 0,
                    temperature: None,
                    temperature_min: discovered.color_temp_min.map(|_| 0),
                    temperature_max: discovered.color_temp_max.map(|_| 100),
                },
                connected: false,
                reachable: false,
                link_quality: None,
                last_seen: None,
                interview_completed: discovered.interview_completed,
            });

            let previous_default_name = default_name_from_friendly_name(&runtime.config.friendly_name);
            let has_custom_name = !runtime.config.name.trim().is_empty() && runtime.config.name != previous_default_name;

            if !has_custom_name {
                runtime.config.name = discovered.name;
            }
            runtime.config.friendly_name = discovered.friendly_name;
            runtime.config.ieee_address = discovered.ieee_address;
            runtime.config.model = discovered.model;
            runtime.config.manufacturer = discovered.manufacturer;
            runtime.config.firmware = discovered.firmware;
            runtime.config.supports_brightness = discovered.supports_brightness;
            runtime.config.supports_temperature = discovered.supports_temperature;
            runtime.config.color_temp_min = discovered.color_temp_min;
            runtime.config.color_temp_max = discovered.color_temp_max;
            runtime.state.temperature_min = runtime.config.color_temp_min.map(|_| 0);
            runtime.state.temperature_max = runtime.config.color_temp_max.map(|_| 100);
            runtime.interview_completed = discovered.interview_completed;
            runtime.connected = discovered.connected;
            runtime.reachable = discovered.reachable;

            if let Some(state) = self
                .inner
                .pending_states
                .write()
                .await
                .remove(&runtime.config.friendly_name)
            {
                apply_state_value(runtime, &state);
            }

            seen.insert(runtime.config.id.clone());
        }

        for lamp in lamps.values_mut() {
            if !seen.contains(&lamp.config.id) {
                lamp.connected = false;
                lamp.reachable = false;
            }
        }

        let stored = lamps.values().map(|lamp| lamp.config.clone()).collect::<Vec<_>>();
        drop(lamps);

        if let Err(error) = self.inner.store.save_lamps(&stored) {
            warn!(error = %error, "failed to persist zigbee lamps");
        }
    }

    async fn handle_availability(&self, friendly_name: &str, payload: &[u8]) {
        let availability = parse_availability(payload);
        let mut lamps = self.inner.lamps.write().await;

        if let Some(lamp) = lamps
            .values_mut()
            .find(|lamp| lamp.config.friendly_name == friendly_name)
        {
            if let Some(is_available) = availability {
                lamp.connected = is_available;
                lamp.reachable = is_available;
            }
        }
    }

    async fn handle_device_state(&self, friendly_name: &str, payload: &[u8]) {
        let Ok(value) = serde_json::from_slice::<Value>(payload) else {
            return;
        };

        let mut lamps = self.inner.lamps.write().await;
        let Some(lamp) = lamps
            .values_mut()
            .find(|lamp| lamp.config.friendly_name == friendly_name)
        else {
            drop(lamps);
            self.inner
                .pending_states
                .write()
                .await
                .insert(friendly_name.to_string(), value);
            return;
        };

        apply_state_value(lamp, &value);
    }

    async fn friendly_name_for(&self, lamp_id: &str) -> Result<String, AppError> {
        let lamps = self.inner.lamps.read().await;
        let lamp = lamps
            .get(lamp_id)
            .ok_or_else(|| not_found("Zigbee lamp not found"))?;
        Ok(lamp.config.friendly_name.clone())
    }

    async fn update_state_after_command<F>(
        &self,
        lamp_id: &str,
        update: F,
    ) -> Result<ZigbeeLampState, AppError>
    where
        F: FnOnce(&mut ZigbeeLampRuntime),
    {
        let mut lamps = self.inner.lamps.write().await;
        let lamp = lamps
            .get_mut(lamp_id)
            .ok_or_else(|| not_found("Zigbee lamp not found"))?;
        update(lamp);
        Ok(current_state(lamp))
    }

    async fn publish_bridge_request(&self, request: &str, payload: Value) -> Result<(), AppError> {
        let client = self.connected_client().await?;
        let topic = format!(
            "{}/bridge/request/{}",
            self.inner.base_topic.trim_end_matches('/'),
            request
        );
        client
            .publish(topic, QoS::AtLeastOnce, false, payload.to_string())
            .await
            .map_err(|error| AppError::service_unavailable(error.to_string()))
    }

    async fn request_devices_refresh(&self) -> Result<(), AppError> {
        self.publish_bridge_request("devices", json!({})).await
    }

    async fn request_states_refresh(&self) -> Result<(), AppError> {
        let lamps = self.inner.lamps.read().await;
        let targets = lamps
            .values()
            .map(|lamp| lamp.config.friendly_name.clone())
            .collect::<Vec<_>>();
        drop(lamps);

        for friendly_name in targets {
            self.publish_device_get(&friendly_name, json!({ "state": "" }))
                .await?;
            self.publish_device_get(&friendly_name, json!({ "brightness": "" }))
                .await?;
        }

        Ok(())
    }

    async fn publish_device_set(&self, friendly_name: &str, payload: Value) -> Result<(), AppError> {
        let client = self.connected_client().await?;
        let topic = format!(
            "{}/{friendly_name}/set",
            self.inner.base_topic.trim_end_matches('/')
        );
        client
            .publish(topic, QoS::AtLeastOnce, false, payload.to_string())
            .await
            .map_err(|error| AppError::service_unavailable(error.to_string()))
    }

    async fn publish_device_get(&self, friendly_name: &str, payload: Value) -> Result<(), AppError> {
        let client = self.connected_client().await?;
        let topic = format!(
            "{}/{friendly_name}/get",
            self.inner.base_topic.trim_end_matches('/')
        );
        client
            .publish(topic, QoS::AtLeastOnce, false, payload.to_string())
            .await
            .map_err(|error| AppError::service_unavailable(error.to_string()))
    }

    async fn connected_client(&self) -> Result<AsyncClient, AppError> {
        self.inner
            .mqtt_client
            .read()
            .await
            .clone()
            .ok_or_else(|| AppError::service_unavailable("Zigbee2MQTT is not connected"))
    }

    async fn set_availability_message(&self, message: Option<String>) {
        let mut slot = self.inner.availability_message.write().await;
        *slot = message;
    }
}

impl ZigbeeStore {
    fn load_lamps(&self) -> Result<Vec<StoredZigbeeLampConfig>, AppError> {
        read_json_file(&self.lamps_path)
    }

    fn save_lamps(&self, lamps: &[StoredZigbeeLampConfig]) -> Result<(), AppError> {
        write_json_file(&self.lamps_path, lamps)
    }

    fn load_blacklist(&self) -> HashSet<String> {
        read_json_file::<Vec<String>>(&self.blacklist_path)
            .unwrap_or_default()
            .into_iter()
            .collect()
    }
}

struct DiscoveredLamp {
    id: String,
    name: String,
    friendly_name: String,
    ieee_address: String,
    model: Option<String>,
    manufacturer: Option<String>,
    firmware: Option<String>,
    supports_brightness: bool,
    supports_temperature: bool,
    color_temp_min: Option<u16>,
    color_temp_max: Option<u16>,
    interview_completed: bool,
    connected: bool,
    reachable: bool,
}

#[derive(Clone)]
struct NativeZigbeeManager {
    inner: Arc<NativeZigbeeManagerInner>,
}

struct NativeZigbeeManagerInner {
    store: ZigbeeStore,
    lamps: RwLock<HashMap<String, ZigbeeLampRuntime>>,
    pairing: RwLock<PairingRuntime>,
    runtime: NativeZigbeeRuntime,
    permit_join_seconds: u16,
    persist_task: Mutex<Option<JoinHandle<()>>>,
}

impl NativeZigbeeManager {
    fn new(config: &Config) -> Result<Self, AppError> {
        let store = ZigbeeStore {
            lamps_path: config.zigbee_lamps_path.clone(),
            blacklist_path: config.zigbee_lamps_blacklist_path.clone(),
        };
        let blacklisted_addresses = store.load_blacklist();
        let lamps: HashMap<String, ZigbeeLampRuntime> = store
            .load_lamps()?
            .into_iter()
            .filter(|lamp| !blacklisted_addresses.contains(&lamp.ieee_address))
            .map(|lamp| {
                let state = RuntimeLampState {
                    is_on: false,
                    brightness: 0,
                    temperature: None,
                    temperature_min: lamp.color_temp_min.map(|_| 0),
                    temperature_max: lamp.color_temp_max.map(|_| 100),
                };

                (
                    lamp.id.clone(),
                    ZigbeeLampRuntime {
                        config: lamp,
                        state,
                        connected: false,
                        reachable: false,
                        link_quality: None,
                        last_seen: None,
                        interview_completed: false,
                    },
                )
            })
            .collect();

        let adapter = env::var("ZIGBEE_ADAPTER").unwrap_or_else(|_| "ember".to_string());
        let serial_port = env::var("ZIGBEE_SERIAL_PORT").ok().and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

        let known_devices = lamps
            .values()
            .filter_map(|lamp| {
                Some(NativeKnownDevice {
                    node_id: lamp.config.node_id?,
                    eui64: lamp.config.ieee_address.clone(),
                    endpoint: lamp.config.endpoint,
                    input_clusters: lamp.config.input_clusters.clone(),
                    output_clusters: lamp.config.output_clusters.clone(),
                    model: lamp.config.model.clone(),
                    manufacturer: lamp.config.manufacturer.clone(),
                    supports_brightness: lamp.config.supports_brightness,
                    supports_temperature: lamp.config.supports_temperature,
                    device_type: if lamp.config.is_remote {
                        ZigbeeDeviceType::Remote
                    } else {
                        ZigbeeDeviceType::Lamp
                    },
                })
            })
            .collect();

        let runtime = NativeZigbeeRuntime::spawn(adapter, serial_port, known_devices);

        let manager = Self {
            inner: Arc::new(NativeZigbeeManagerInner {
                store,
                lamps: RwLock::new(lamps),
                pairing: RwLock::new(PairingRuntime::default()),
                runtime,
                permit_join_seconds: config.zigbee_permit_join_seconds,
                persist_task: Mutex::new(None),
            }),
        };

        manager.spawn_persist_task();
        Ok(manager)
    }

    async fn list_lamps(&self) -> Vec<ZigbeeLampView> {
        if let Err(error) = self.sync_from_runtime().await {
            warn!(error = %error, "failed to sync native zigbee lamps before listing");
        }
        let lamps = self.inner.lamps.read().await;
        let mut values = lamps
            .values()
            .filter(|lamp| !lamp.config.is_remote && lamp.interview_completed)
            .map(to_view)
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.name.cmp(&right.name));
        values
    }

    async fn get_lamp(&self, lamp_id: &str) -> Option<ZigbeeLampView> {
        if let Err(error) = self.sync_from_runtime().await {
            warn!(lamp_id, error = %error, "failed to sync native zigbee lamp before loading it");
        }
        let lamps = self.inner.lamps.read().await;
        lamps.get(lamp_id).map(to_view)
    }

    async fn stats(&self) -> ZigbeeStats {
        if let Err(error) = self.sync_from_runtime().await {
            warn!(error = %error, "failed to sync native zigbee stats");
        }
        let lamps = self.inner.lamps.read().await;
        ZigbeeStats {
            total: lamps.values().filter(|lamp| !lamp.config.is_remote && lamp.interview_completed).count(),
            connected: lamps.values().filter(|lamp| !lamp.config.is_remote && lamp.interview_completed && lamp.connected).count(),
            reachable: lamps.values().filter(|lamp| !lamp.config.is_remote && lamp.interview_completed && lamp.reachable).count(),
            disabled: false,
            message: self.inner.runtime.message().await,
        }
    }

    async fn pairing_status(&self) -> ZigbeePairingStatus {
        let mut pairing = self.inner.pairing.write().await;
        let remaining_seconds = remaining_seconds(&mut pairing);
        let fallback_message = self.inner.runtime.message().await;

        ZigbeePairingStatus {
            active: pairing.active,
            remaining_seconds,
            permit_join_seconds: self.inner.permit_join_seconds,
            message: pairing
                .message
                .clone()
                .or(fallback_message),
        }
    }

    async fn start_pairing(&self) -> Result<ZigbeePairingStatus, AppError> {
        let seconds = self.inner.permit_join_seconds;
        self.inner
            .runtime
            .send(NativeZigbeeCommand::PermitJoin { seconds })
            .await?;

        let mut pairing = self.inner.pairing.write().await;
        pairing.active = true;
        pairing.deadline = Some(Instant::now() + Duration::from_secs(u64::from(seconds)));
        pairing.message = Some("Native Zigbee pairing window requested".to_string());
        let remaining_seconds = remaining_seconds(&mut pairing);

        Ok(ZigbeePairingStatus {
            active: pairing.active,
            remaining_seconds,
            permit_join_seconds: seconds,
            message: pairing.message.clone(),
        })
    }

    async fn stop_pairing(&self) -> Result<ZigbeePairingStatus, AppError> {
        self.inner
            .runtime
            .send(NativeZigbeeCommand::PermitJoin { seconds: 0 })
            .await?;

        let mut pairing = self.inner.pairing.write().await;
        pairing.active = false;
        pairing.deadline = None;
        pairing.message = Some("Native Zigbee pairing window closed".to_string());

        Ok(ZigbeePairingStatus {
            active: false,
            remaining_seconds: 0,
            permit_join_seconds: self.inner.permit_join_seconds,
            message: pairing.message.clone(),
        })
    }

    async fn set_power(&self, lamp_id: &str, enabled: bool) -> Result<ZigbeeLampState, AppError> {
        if let Err(error) = self.sync_from_runtime().await {
            warn!(lamp_id, error = %error, "failed to sync native zigbee lamp before power change");
        }
        self.inner
            .runtime
            .send(NativeZigbeeCommand::SetPower {
                lamp_id: lamp_id.to_string(),
                enabled,
            })
            .await?;
        tokio::time::sleep(Duration::from_millis(250)).await;
        self.sync_from_runtime().await?;
        self.current_state(lamp_id).await
    }

    async fn set_brightness(&self, lamp_id: &str, brightness: u8) -> Result<ZigbeeLampState, AppError> {
        if let Err(error) = self.sync_from_runtime().await {
            warn!(lamp_id, error = %error, "failed to sync native zigbee lamp before brightness change");
        }
        self.inner
            .runtime
            .send(NativeZigbeeCommand::SetBrightness {
                lamp_id: lamp_id.to_string(),
                brightness,
            })
            .await?;
        tokio::time::sleep(Duration::from_millis(250)).await;
        self.sync_from_runtime().await?;
        self.current_state(lamp_id).await
    }

    async fn set_temperature(&self, lamp_id: &str, temperature: u8) -> Result<ZigbeeLampState, AppError> {
        if let Err(error) = self.sync_from_runtime().await {
            warn!(lamp_id, error = %error, "failed to sync native zigbee lamp before temperature change");
        }
        self.inner
            .runtime
            .send(NativeZigbeeCommand::SetTemperature {
                lamp_id: lamp_id.to_string(),
                temperature,
            })
            .await?;
        tokio::time::sleep(Duration::from_millis(250)).await;
        self.sync_from_runtime().await?;
        self.current_state(lamp_id).await
    }

    async fn rename_lamp(&self, lamp_id: &str, name: &str) -> Result<(), AppError> {
        if let Err(error) = self.sync_from_runtime().await {
            warn!(lamp_id, error = %error, "failed to sync native zigbee lamp before rename");
        }
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AppError::http(
                StatusCode::BAD_REQUEST,
                "Lamp name cannot be empty",
            ));
        }

        let stored = {
            let mut lamps = self.inner.lamps.write().await;
            let lamp = lamps
                .get_mut(lamp_id)
                .ok_or_else(|| not_found("Zigbee lamp not found"))?;
            lamp.config.name = trimmed.to_string();
            lamps.values().map(|lamp| lamp.config.clone()).collect::<Vec<_>>()
        };

        self.inner.store.save_lamps(&stored)
    }

    async fn shutdown(&self) {
        if let Some(handle) = self.inner.persist_task.lock().expect("native persist task mutex").take() {
            handle.abort();
        }
        self.inner.runtime.shutdown().await;
    }

    async fn sync_from_runtime(&self) -> Result<(), AppError> {
        self.inner.runtime.ensure_initialized().await;
        let discovered = self.inner.runtime.snapshot_devices().await;

        let mut lamps = self.inner.lamps.write().await;
        let mut seen = HashSet::new();
        let mut changed = false;

        for device in discovered {
            // Skip devices that haven't completed their interview or discovery yet (no
            // endpoint means we don't know what the device is — it could be a
            // sleepy remote still being discovered).
            // Remotes are included so they get persisted to disk and survive
            // coordinator reboots.  They are filtered out at API/display time
            // in list_lamps() and stats() via the is_remote flag.
            if device.endpoint.is_none() {
                continue;
            }

            let id = device.id.clone();
            let previous = lamps.get(&id).cloned();
            let runtime = lamps.entry(id.clone()).or_insert_with(|| ZigbeeLampRuntime {
                config: StoredZigbeeLampConfig {
                    id: id.clone(),
                    name: device.eui64.clone(),
                    friendly_name: device.eui64.clone(),
                    ieee_address: device.eui64.clone(),
                    node_id: Some(device.node_id),
                    endpoint: device.endpoint,
                    input_clusters: device.input_clusters.clone(),
                    output_clusters: device.output_clusters.clone(),
                    model: device.model.clone(),
                    manufacturer: device.manufacturer.clone().or_else(|| Some("Native EZSP".to_string())),
                    firmware: None,
                    supports_brightness: device.supports_brightness,
                    supports_temperature: device.supports_temperature,
                    color_temp_min: if device.supports_temperature { Some(153) } else { None },
                    color_temp_max: if device.supports_temperature { Some(500) } else { None },
                    is_remote: device.device_type == ZigbeeDeviceType::Remote,
                },
                state: RuntimeLampState {
                    is_on: device.is_on,
                    brightness: device.brightness,
                    temperature: device.temperature,
                    temperature_min: if device.supports_temperature { Some(0) } else { None },
                    temperature_max: if device.supports_temperature { Some(100) } else { None },
                },
                connected: device.connected,
                reachable: device.reachable,
                link_quality: None,
                last_seen: device.last_seen.clone(),
                interview_completed: device.endpoint.is_some(),
            });

            runtime.config.ieee_address = device.eui64.clone();
            runtime.config.node_id = Some(device.node_id);
            runtime.config.endpoint = device.endpoint;
            runtime.config.input_clusters = device.input_clusters.clone();
            runtime.config.output_clusters = device.output_clusters.clone();
            runtime.config.model = device.model.clone().or(runtime.config.model.clone());
            runtime.config.manufacturer = device.manufacturer.clone().or(runtime.config.manufacturer.clone());
            runtime.config.supports_brightness = device.supports_brightness;
            runtime.config.supports_temperature = device.supports_temperature;
            runtime.config.color_temp_min = if device.supports_temperature { Some(153) } else { None };
            runtime.config.color_temp_max = if device.supports_temperature { Some(500) } else { None };
            runtime.config.is_remote = device.device_type == ZigbeeDeviceType::Remote;
            runtime.connected = device.connected;
            runtime.reachable = device.reachable;
            runtime.interview_completed = device.endpoint.is_some();
            if device.last_seen.is_some() {
                runtime.last_seen = device.last_seen.clone();
            }
            if device.connected {
                runtime.state.is_on = device.is_on;
                runtime.state.brightness = device.brightness;
            }
            runtime.state.temperature = device.temperature;
            runtime.state.temperature_min = if device.supports_temperature { Some(0) } else { None };
            runtime.state.temperature_max = if device.supports_temperature { Some(100) } else { None };
            if runtime.config.name.trim().is_empty() {
                runtime.config.name = device.eui64.clone();
            }
            if runtime.config.friendly_name.trim().is_empty() {
                runtime.config.friendly_name = device.eui64.clone();
            }

            if previous.map(|value| native_runtime_equals(&value, runtime)).unwrap_or(false) == false {
                changed = true;
            }

            seen.insert(id);
        }

        for lamp in lamps.values_mut() {
            if !seen.contains(&lamp.config.id) {
                // Don't mark remotes as disconnected — they are sleepy end
                // devices and may not appear in every snapshot.
                if lamp.config.is_remote {
                    continue;
                }
                if lamp.connected || lamp.reachable {
                    changed = true;
                }
                lamp.connected = false;
                lamp.reachable = false;
            }
        }

        if changed {
            let stored = lamps.values().map(|lamp| lamp.config.clone()).collect::<Vec<_>>();
            drop(lamps);
            self.inner.store.save_lamps(&stored)?;
        }

        Ok(())
    }

    fn spawn_persist_task(&self) {
        if self.inner.persist_task.lock().expect("native persist task mutex").is_some() {
            return;
        }

        let manager = self.clone();
        let handle = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(2));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tick.tick().await;
                if let Err(error) = manager.sync_from_runtime().await {
                    warn!(error = %error, "failed to persist native zigbee runtime state");
                }
            }
        });

        *self.inner.persist_task.lock().expect("native persist task mutex") = Some(handle);
    }

    async fn current_state(&self, lamp_id: &str) -> Result<ZigbeeLampState, AppError> {
        let lamps = self.inner.lamps.read().await;
        let lamp = lamps
            .get(lamp_id)
            .ok_or_else(|| not_found("Zigbee lamp not found"))?;
        Ok(current_state(lamp))
    }

}

fn native_runtime_equals(left: &ZigbeeLampRuntime, right: &ZigbeeLampRuntime) -> bool {
    left.config.node_id == right.config.node_id
        && left.config.endpoint == right.config.endpoint
        && left.config.input_clusters == right.config.input_clusters
        && left.config.output_clusters == right.config.output_clusters
        && left.config.supports_brightness == right.config.supports_brightness
        && left.config.supports_temperature == right.config.supports_temperature
        && left.connected == right.connected
        && left.reachable == right.reachable
        && left.state.is_on == right.state.is_on
        && left.state.brightness == right.state.brightness
        && left.state.temperature == right.state.temperature
}

impl DiscoveredLamp {
    fn from_bridge_device(value: &Value) -> Option<Self> {
        let friendly_name = value.get("friendly_name")?.as_str()?.to_string();
        if friendly_name == "Coordinator" {
            return None;
        }

        let ieee_address = value.get("ieee_address")?.as_str()?.to_string();
        let interview_completed = value
            .get("interview_completed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let disabled = value.get("disabled").and_then(Value::as_bool).unwrap_or(false);
        let definition = value.get("definition").unwrap_or(&Value::Null);
        let exposes = definition.get("exposes").unwrap_or(&Value::Null);

        if !is_supported_lamp(exposes) {
            return None;
        }

        let supports_brightness = has_property(exposes, "brightness");
        let supports_temperature = has_property(exposes, "color_temp");
        let (color_temp_min, color_temp_max) = extract_numeric_range(exposes, "color_temp")
            .map(|(min, max)| (Some(min), Some(max)))
            .unwrap_or((None, None));

        let model = definition
            .get("model")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                value.get("model_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            });

        let manufacturer = definition
            .get("vendor")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                value.get("manufacturer")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            });

        let firmware = value
            .get("software_build_id")
            .and_then(Value::as_str)
            .map(ToString::to_string);

        Some(Self {
            id: normalize_id(&ieee_address),
            name: friendly_name.replace('_', " "),
            friendly_name,
            ieee_address,
            model,
            manufacturer,
            firmware,
            supports_brightness,
            supports_temperature,
            color_temp_min,
            color_temp_max,
            interview_completed,
            connected: !disabled && interview_completed,
            reachable: !disabled && interview_completed,
        })
    }
}

fn to_view(lamp: &ZigbeeLampRuntime) -> ZigbeeLampView {
    ZigbeeLampView {
        id: lamp.config.id.clone(),
        name: lamp.config.name.clone(),
        address: lamp.config.ieee_address.clone(),
        friendly_name: lamp.config.friendly_name.clone(),
        link_quality: lamp.link_quality,
        interview_completed: lamp.interview_completed,
        model: lamp.config.model.clone(),
        manufacturer: lamp
            .config
            .manufacturer
            .clone()
            .unwrap_or_else(|| "Unknown".to_string()),
        firmware: lamp.config.firmware.clone(),
        connected: lamp.connected,
        reachable: lamp.reachable,
        supports_brightness: lamp.config.supports_brightness,
        supports_temperature: lamp.config.supports_temperature,
        state: current_state(lamp),
        last_seen: lamp.last_seen.clone(),
    }
}

fn current_state(lamp: &ZigbeeLampRuntime) -> ZigbeeLampState {
    ZigbeeLampState {
        is_on: lamp.state.is_on,
        brightness: lamp.state.brightness,
        temperature: lamp.state.temperature,
        temperature_min: lamp.state.temperature_min,
        temperature_max: lamp.state.temperature_max,
    }
}

fn apply_state_value(lamp: &mut ZigbeeLampRuntime, value: &Value) {
    debug!(payload = %value, lamp = %lamp.config.friendly_name, "applying zigbee state payload");
    if let Some(state) = value.get("state").and_then(Value::as_str) {
        lamp.state.is_on = state.eq_ignore_ascii_case("on");
    }

    if let Some(brightness) = value.get("brightness").and_then(value_as_u16) {
        lamp.state.brightness = parse_brightness(brightness);
    }

    if lamp.config.supports_temperature {
        if let Some(raw_temperature) = value.get("color_temp").and_then(value_as_u16) {
            lamp.state.temperature = Some(parse_temperature(
                raw_temperature,
                lamp.config.color_temp_min.unwrap_or(153),
                lamp.config.color_temp_max.unwrap_or(500),
            ));
        }
    } else {
        lamp.state.temperature = None;
    }

    if let Some(link_quality) = value.get("linkquality").and_then(value_as_u16) {
        lamp.link_quality = Some(link_quality);
    }

    if let Some(last_seen) = value.get("last_seen") {
        lamp.last_seen = stringify_value(last_seen);
    }

    if let Some(is_available) = value.get("availability").and_then(availability_from_value) {
        lamp.connected = is_available;
        lamp.reachable = is_available;
    } else {
        lamp.connected = true;
        lamp.reachable = true;
    }
}

fn bridge_devices_entries<'a>(value: &'a Value) -> Option<&'a Vec<Value>> {
    value
        .as_array()
        .or_else(|| value.get("data").and_then(Value::as_array))
}

fn remaining_seconds(pairing: &mut PairingRuntime) -> u16 {
    if !pairing.active {
        pairing.deadline = None;
        return 0;
    }

    let Some(deadline) = pairing.deadline else {
        pairing.active = false;
        return 0;
    };

    let now = Instant::now();
    if deadline <= now {
        pairing.active = false;
        pairing.deadline = None;
        return 0;
    }

    deadline.saturating_duration_since(now).as_secs().min(u16::MAX as u64) as u16
}

fn is_supported_lamp(exposes: &Value) -> bool {
    (has_property(exposes, "state") && is_light_type(exposes))
        || (has_property(exposes, "state") && has_property(exposes, "brightness"))
}

fn is_light_type(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(is_light_type),
        Value::Object(map) => {
            map.get("type").and_then(Value::as_str) == Some("light")
                || map.values().any(is_light_type)
        }
        _ => false,
    }
}

fn has_property(value: &Value, property: &str) -> bool {
    match value {
        Value::Array(values) => values.iter().any(|value| has_property(value, property)),
        Value::Object(map) => {
            map.get("property").and_then(Value::as_str) == Some(property)
                || map.values().any(|value| has_property(value, property))
        }
        _ => false,
    }
}

fn extract_numeric_range(value: &Value, property: &str) -> Option<(u16, u16)> {
    match value {
        Value::Array(values) => values
            .iter()
            .find_map(|value| extract_numeric_range(value, property)),
        Value::Object(map) => {
            if map.get("property").and_then(Value::as_str) == Some(property) {
                let min = map
                    .get("value_min")
                    .or_else(|| map.get("min"))
                    .and_then(value_as_u16)?;
                let max = map
                    .get("value_max")
                    .or_else(|| map.get("max"))
                    .and_then(value_as_u16)?;
                return Some((min, max));
            }

            map.values()
                .find_map(|value| extract_numeric_range(value, property))
        }
        _ => None,
    }
}

fn parse_availability(payload: &[u8]) -> Option<bool> {
    let Ok(value) = serde_json::from_slice::<Value>(payload) else {
        let state = String::from_utf8_lossy(payload).trim().to_ascii_lowercase();
        return match state.as_str() {
            "online" => Some(true),
            "offline" => Some(false),
            _ => None,
        };
    };

    availability_from_value(&value)
}

fn availability_from_value(value: &Value) -> Option<bool> {
    if let Some(state) = value.as_str() {
        return match state.to_ascii_lowercase().as_str() {
            "online" | "on" => Some(true),
            "offline" | "off" => Some(false),
            _ => None,
        };
    }

    value
        .get("state")
        .and_then(Value::as_str)
        .and_then(|state| match state.to_ascii_lowercase().as_str() {
            "online" | "on" => Some(true),
            "offline" | "off" => Some(false),
            _ => None,
        })
}

fn parse_brightness(raw_value: u16) -> u8 {
    let clamped = raw_value.clamp(0, 254);
    (((clamped * 100) + 127) / 254) as u8
}

fn to_brightness(percentage: u8) -> u16 {
    let clamped = percentage.clamp(0, 100);
    ((u16::from(clamped) * 254) / 100).max(1)
}

fn parse_temperature(raw_value: u16, min: u16, max: u16) -> u8 {
    if min >= max {
        return 0;
    }

    let clamped = raw_value.clamp(min, max);
    let span = max - min;
    (((max - clamped) * 100) / span) as u8
}

fn to_temperature(percentage: u8, min: u16, max: u16) -> u16 {
    if min >= max {
        return min;
    }

    let clamped = u16::from(percentage.clamp(0, 100));
    max.saturating_sub((clamped * (max - min)) / 100)
}

fn normalize_id(value: &str) -> String {
    let normalized = value
        .chars()
        .filter(|character| character.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase();

    if normalized.is_empty() {
        format!("zigbee-{:x}", md5::compute(value))
    } else {
        normalized
    }
}

fn default_name_from_friendly_name(friendly_name: &str) -> String {
    friendly_name.replace('_', " ")
}

fn value_as_u16(value: &Value) -> Option<u16> {
    value.as_u64().map(|number| number.min(u16::MAX as u64) as u16)
}

fn stringify_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        _ => Some(value.to_string()),
    }
}

fn read_json_file<T>(path: &Path) -> Result<T, AppError>
where
    T: for<'de> Deserialize<'de> + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }

    let body = fs::read(path)?;
    if body.is_empty() {
        return Ok(T::default());
    }

    Ok(serde_json::from_slice(&body)?)
}

fn write_json_file<T>(path: &Path, value: &T) -> Result<(), AppError>
where
    T: Serialize + ?Sized,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let body = serde_json::to_vec_pretty(value)?;
    fs::write(path, body)?;
    Ok(())
}

fn not_found(message: impl Into<String>) -> AppError {
    AppError::http(StatusCode::NOT_FOUND, message)
}

fn client_error_to_string(error: ClientError) -> String {
    error.to_string()
}

fn connection_error_to_string(error: ConnectionError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::{NativeZigbeeManager, StoredZigbeeLampConfig, default_name_from_friendly_name};
    use crate::{config::Config, zigbee_native::{DriverLifecycle, NativeDiscoveredDevice, ZigbeeDeviceType}};
    use tempfile::tempdir;

    #[test]
    fn derived_default_name_replaces_underscores() {
        assert_eq!(default_name_from_friendly_name("0x00158d0001_abcd"), "0x00158d0001 abcd");
    }

    #[test]
    fn custom_name_detection_distinguishes_default_and_custom_values() {
        let previous_default_name = default_name_from_friendly_name("kitchen_light");

        let default_name = "kitchen light";
        let custom_name = "Lampe cuisine";

        let default_is_custom = !default_name.trim().is_empty() && default_name != previous_default_name;
        let custom_is_custom = !custom_name.trim().is_empty() && custom_name != previous_default_name;

        assert!(!default_is_custom);
        assert!(custom_is_custom);
    }

    #[tokio::test]
    async fn native_set_power_reaches_runtime_send_path() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();

        std::fs::create_dir_all(root.join("frontend/dist")).expect("frontend dist");
        std::fs::write(root.join("frontend/dist/index.html"), "ok").expect("index");
        std::fs::write(root.join("users.json"), "[]").expect("users");
        std::fs::write(root.join("devices.json"), "[]").expect("devices");
        std::fs::write(root.join("device-cache.json"), "[]").expect("device-cache");
        std::fs::write(root.join("broadlink-codes.json"), r#"{"codes":[]}"#).expect("broadlink");
        std::fs::write(root.join("meross-devices.json"), "[]").expect("meross");
        std::fs::write(root.join("hue-lamps.json"), "[]").expect("hue");
        std::fs::write(root.join("hue-lamps-blacklist.json"), "[]").expect("hue blacklist");
        std::fs::write(root.join("zigbee-lamps-blacklist.json"), "[]").expect("zigbee blacklist");
        std::fs::write(
            root.join("zigbee-lamps.json"),
            serde_json::to_string(&vec![StoredZigbeeLampConfig {
                id: "4b8ec60801881700".to_string(),
                name: "Test Lamp".to_string(),
                friendly_name: "4b8ec60801881700".to_string(),
                ieee_address: "4b:8e:c6:08:01:88:17:00".to_string(),
                node_id: Some(0x2e34),
                endpoint: Some(11),
                input_clusters: vec![0, 3, 4, 5, 6, 8],
                output_clusters: vec![25],
                model: Some("LTG002".to_string()),
                manufacturer: Some("Signify Netherlands B.V.".to_string()),
                firmware: None,
                supports_brightness: true,
                supports_temperature: false,
                color_temp_min: None,
                color_temp_max: None,
                is_remote: false,
            }])
            .expect("serialize zigbee lamps"),
        )
        .expect("zigbee lamps");

        let mut config = Config::for_tests(root.to_path_buf());
        std::env::set_var("ZIGBEE_BACKEND", "native");
        std::env::set_var("ZIGBEE_NATIVE_ADAPTER", "ember");
        std::env::set_var("ZIGBEE_SERIAL_PORT", "/dev/null");
        let manager = NativeZigbeeManager::new(&config).expect("native manager");
        config.zigbee_permit_join_seconds = 120;

        manager.inner.runtime.test_seed_devices(vec![NativeDiscoveredDevice {
                id: "4b8ec60801881700".to_string(),
                node_id: 0x2e34,
                eui64: "4b:8e:c6:08:01:88:17:00".to_string(),
                endpoint: Some(11),
                input_clusters: vec![0, 3, 4, 5, 6, 8],
                output_clusters: vec![25],
                supports_brightness: true,
                supports_temperature: false,
                device_type: ZigbeeDeviceType::Lamp,
                connected: true,
                reachable: true,
                is_on: true,
                brightness: 100,
                temperature: None,
                model: Some("LTG002".to_string()),
                manufacturer: Some("Signify Netherlands B.V.".to_string()),
                last_seen: None,
        }]).await;
        manager.inner.runtime.test_set_lifecycle(DriverLifecycle::Failed("boom".to_string())).await;
        manager.inner.runtime.test_set_network_state("joined").await;

        let error = manager
            .set_power("4b8ec60801881700", false)
            .await
            .expect_err("power change should surface runtime failure");

        assert!(error.to_string().contains("boom"), "unexpected error: {error}");
    }
}
