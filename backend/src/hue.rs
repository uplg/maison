use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use btleplug::{
    api::{
        Central, CentralEvent, CharPropFlags, Characteristic, Manager as _, Peripheral as _,
        PeripheralProperties, ScanFilter, ValueNotification, WriteType,
    },
    platform::{Adapter, Manager as BleManager, Peripheral},
};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::{Duration, sleep, timeout},
};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{config::Config, error::AppError};

const SCAN_INTERVAL: Duration = Duration::from_secs(10);
const SCAN_DURATION: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_NEW_LAMP_FAILURES: u8 = 3;

const LIGHT_CONTROL_SERVICE: &str = "932c32bd-0000-47a2-835a-a8d455b859dd";
const POWER_UUID: &str = "932c32bd-0002-47a2-835a-a8d455b859dd";
const BRIGHTNESS_UUID: &str = "932c32bd-0003-47a2-835a-a8d455b859dd";
const TEMPERATURE_UUID: &str = "932c32bd-0004-47a2-835a-a8d455b859dd";
const CONTROL_UUID: &str = "932c32bd-0007-47a2-835a-a8d455b859dd";
const MODEL_UUID: &str = "00002a24-0000-1000-8000-00805f9b34fb";
const FIRMWARE_UUID: &str = "00002a28-0000-1000-8000-00805f9b34fb";
const MANUFACTURER_UUID: &str = "00002a29-0000-1000-8000-00805f9b34fb";
const DEVICE_NAME_UUID: &str = "97fe6561-0003-4f62-86e9-b71ee2da3d22";
const CONFIG_SERVICE_UUID: &str = "0000fe0f-0000-1000-8000-00805f9b34fb";

const PHILIPS_MANUFACTURER_ID: u16 = 0x0075;
const SIGNIFY_MANUFACTURER_ID: u16 = 0x0105;
const ZERO_BLE_ADDRESS: &str = "00:00:00:00:00:00";

#[derive(Debug, Clone, Serialize)]
pub struct HueLampState {
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
pub struct HueLampView {
    pub id: String,
    pub name: String,
    pub address: String,
    pub model: Option<String>,
    pub manufacturer: String,
    pub firmware: Option<String>,
    pub connected: bool,
    pub connecting: bool,
    pub reachable: bool,
    pub state: HueLampState,
    #[serde(rename = "lastSeen")]
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HueStats {
    pub total: usize,
    pub connected: usize,
    pub reachable: usize,
    pub disabled: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredLampConfig {
    id: String,
    name: String,
    address: String,
    model: Option<String>,
    #[serde(default)]
    has_connected_once: bool,
    temperature_min: Option<u8>,
    temperature_max: Option<u8>,
    last_temperature: Option<u8>,
}

#[derive(Clone)]
pub struct HueManager {
    inner: Arc<HueManagerInner>,
}

struct HueManagerInner {
    store: HueStore,
    lamps: RwLock<HashMap<String, LampRuntime>>,
    blacklisted_addresses: RwLock<HashSet<String>>,
    adapter: RwLock<Option<Adapter>>,
    availability_message: RwLock<Option<String>>,
    disabled: bool,
    shutting_down: AtomicBool,
    ble_lock: Mutex<()>,
    scan_task: Mutex<Option<JoinHandle<()>>>,
    poll_task: Mutex<Option<JoinHandle<()>>>,
    event_task: Mutex<Option<JoinHandle<()>>>,
}

struct LampRuntime {
    config: StoredLampConfig,
    state: RuntimeLampState,
    info: LampInfo,
    connected: bool,
    connecting: bool,
    reachable: bool,
    last_seen: Option<DateTime<Utc>>,
    peripheral: Option<Peripheral>,
    characteristics: HueCharacteristics,
    connection_failures: u8,
    notification_task: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct RuntimeLampState {
    is_on: bool,
    brightness: u8,
    temperature: Option<u8>,
    temperature_min: Option<u8>,
    temperature_max: Option<u8>,
}

#[derive(Clone)]
struct LampInfo {
    manufacturer: String,
    firmware: Option<String>,
    model: Option<String>,
}

#[derive(Clone, Default)]
struct HueCharacteristics {
    power: Option<Characteristic>,
    brightness: Option<Characteristic>,
    temperature: Option<Characteristic>,
    control: Option<Characteristic>,
    model: Option<Characteristic>,
    firmware: Option<Characteristic>,
    manufacturer: Option<Characteristic>,
    device_name: Option<Characteristic>,
}

#[derive(Clone)]
struct HueStore {
    lamps_path: PathBuf,
    blacklist_path: PathBuf,
}

#[derive(Clone)]
struct ConnectionReady {
    peripheral: Peripheral,
    characteristics: HueCharacteristics,
    info: LampInfo,
    state: RuntimeLampState,
}

impl HueManager {
    pub fn new(config: &Config) -> Result<Self, AppError> {
        let store = HueStore {
            lamps_path: config.hue_lamps_path.clone(),
            blacklist_path: config.hue_blacklist_path.clone(),
        };

        let stored_configs = store.load_lamps();
        let blacklisted_addresses = store.load_blacklist();
        let lamps = stored_configs
            .into_iter()
            .map(|lamp| {
                let state = RuntimeLampState {
                    is_on: false,
                    brightness: 100,
                    temperature: lamp.last_temperature,
                    temperature_min: lamp.temperature_min,
                    temperature_max: lamp.temperature_max,
                };
                let info = LampInfo {
                    manufacturer: "Philips Hue".to_string(),
                    firmware: None,
                    model: lamp.model.clone(),
                };
                (
                    lamp.id.clone(),
                    LampRuntime {
                        config: lamp,
                        state,
                        info,
                        connected: false,
                        connecting: false,
                        reachable: false,
                        last_seen: None,
                        peripheral: None,
                        characteristics: HueCharacteristics::default(),
                        connection_failures: 0,
                        notification_task: None,
                    },
                )
            })
            .collect();

        let manager = Self {
            inner: Arc::new(HueManagerInner {
                store,
                lamps: RwLock::new(lamps),
                blacklisted_addresses: RwLock::new(blacklisted_addresses),
                adapter: RwLock::new(None),
                availability_message: RwLock::new(if config.disable_bluetooth {
                    Some("Bluetooth is disabled in this environment".to_string())
                } else {
                    Some("Bluetooth adapter initializing".to_string())
                }),
                disabled: config.disable_bluetooth,
                shutting_down: AtomicBool::new(false),
                ble_lock: Mutex::new(()),
                scan_task: Mutex::new(None),
                poll_task: Mutex::new(None),
                event_task: Mutex::new(None),
            }),
        };

        if !config.disable_bluetooth {
            let init_manager = manager.clone();
            tokio::spawn(async move {
                init_manager.initialize_runtime().await;
            });
        }

        Ok(manager)
    }

    pub async fn shutdown(&self) {
        self.inner.shutting_down.store(true, Ordering::SeqCst);

        if let Some(task) = self.inner.scan_task.lock().await.take() {
            task.abort();
        }
        if let Some(task) = self.inner.poll_task.lock().await.take() {
            task.abort();
        }
        if let Some(task) = self.inner.event_task.lock().await.take() {
            task.abort();
        }

        let lamp_ids = {
            let lamps = self.inner.lamps.read().await;
            lamps.keys().cloned().collect::<Vec<_>>()
        };

        for lamp_id in lamp_ids {
            let _ = self.disconnect_lamp(&lamp_id).await;
        }
    }

    pub async fn list_lamps(&self) -> Vec<HueLampView> {
        let lamps = self.inner.lamps.read().await;
        lamps.values().map(to_view).collect()
    }

    pub async fn get_lamp(&self, lamp_id: &str) -> Option<HueLampView> {
        let lamp_key = self.resolve_lamp_key(lamp_id).await?;
        let lamps = self.inner.lamps.read().await;
        lamps.get(&lamp_key).map(to_view)
    }

    pub async fn stats(&self) -> HueStats {
        let lamps = self.inner.lamps.read().await;
        let total = lamps.len();
        let connected = lamps.values().filter(|lamp| lamp.connected).count();
        let reachable = lamps.values().filter(|lamp| lamp.reachable).count();
        let message = self.inner.availability_message.read().await.clone();

        HueStats {
            total,
            connected,
            reachable,
            disabled: self.inner.disabled,
            message,
        }
    }

    pub async fn trigger_scan(&self) -> Result<(), AppError> {
        self.perform_scan(SCAN_DURATION).await
    }

    pub async fn connect_all(&self) {
        let lamp_ids = {
            let lamps = self.inner.lamps.read().await;
            lamps.keys().cloned().collect::<Vec<_>>()
        };

        for lamp_id in lamp_ids {
            let _ = self.connect_lamp(&lamp_id).await;
        }
    }

    pub async fn disconnect_all(&self) {
        let lamp_ids = {
            let lamps = self.inner.lamps.read().await;
            lamps.keys().cloned().collect::<Vec<_>>()
        };

        for lamp_id in lamp_ids {
            let _ = self.disconnect_lamp(&lamp_id).await;
        }
    }

    pub async fn connect_lamp(&self, lamp_id: &str) -> Result<bool, AppError> {
        if self.inner.disabled {
            return Ok(false);
        }

        let lamp_key = self
            .resolve_lamp_key(lamp_id)
            .await
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"))?;

        let peripheral = {
            let mut lamps = self.inner.lamps.write().await;
            let Some(lamp) = lamps.get_mut(&lamp_key) else {
                return Err(AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"));
            };
            if lamp.connected || lamp.connecting {
                return Ok(lamp.connected);
            }
            lamp.connecting = true;
            lamp.peripheral.clone()
        };

        let Some(peripheral) = peripheral else {
            self.mark_connect_failure(&lamp_key).await?;
            return Ok(false);
        };

        let result = self.establish_connection(&lamp_key, peripheral.clone()).await;
        match result {
            Ok(ready) => {
                self.finish_successful_connection(&lamp_key, ready).await?;
                Ok(true)
            }
            Err(error) => {
                self.mark_connect_failure(&lamp_key).await?;
                debug!(lamp_id, error = %error, "Hue lamp connection failed");
                Ok(false)
            }
        }
    }

    pub async fn disconnect_lamp(&self, lamp_id: &str) -> Result<(), AppError> {
        let lamp_key = self
            .resolve_lamp_key(lamp_id)
            .await
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"))?;

        let (peripheral, notification_task) = {
            let mut lamps = self.inner.lamps.write().await;
            let Some(lamp) = lamps.get_mut(&lamp_key) else {
                return Err(AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"));
            };
            lamp.connected = false;
            lamp.connecting = false;
            lamp.reachable = false;
            lamp.characteristics = HueCharacteristics::default();
            (lamp.peripheral.clone(), lamp.notification_task.take())
        };

        if let Some(task) = notification_task {
            task.abort();
        }

        if let Some(peripheral) = peripheral {
            let _guard = self.inner.ble_lock.lock().await;
            if matches!(peripheral.is_connected().await, Ok(true)) {
                let _ = timeout(IO_TIMEOUT, peripheral.disconnect()).await;
            }
        }

        Ok(())
    }

    pub async fn refresh_lamp_state(&self, lamp_id: &str) -> Result<Option<HueLampState>, AppError> {
        let lamp_key = self
            .resolve_lamp_key(lamp_id)
            .await
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"))?;

        let (peripheral, characteristics) = {
            let lamps = self.inner.lamps.read().await;
            let Some(lamp) = lamps.get(&lamp_key) else {
                return Err(AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"));
            };
            if !lamp.connected {
                return Ok(None);
            }
            (lamp.peripheral.clone(), lamp.characteristics.clone())
        };

        let Some(peripheral) = peripheral else {
            return Ok(None);
        };

        let state = self.read_state(&peripheral, &characteristics).await?;
        {
            let mut lamps = self.inner.lamps.write().await;
            if let Some(lamp) = lamps.get_mut(&lamp_key) {
                lamp.state = state.clone();
                lamp.connected = true;
                lamp.reachable = true;
            }
        }

        Ok(Some(HueLampState {
            is_on: state.is_on,
            brightness: state.brightness,
            temperature: state.temperature,
            temperature_min: state.temperature_min,
            temperature_max: state.temperature_max,
        }))
    }

    pub async fn set_power(&self, lamp_id: &str, enabled: bool) -> Result<HueLampState, AppError> {
        let (peripheral, characteristics) = self.connected_target(lamp_id).await?;
        if let Some(power) = characteristics.power.as_ref() {
            self.write_characteristic(&peripheral, power, &[u8::from(enabled)]).await?;
        } else if let Some(control) = characteristics.control.as_ref() {
            let command = build_control_command(Some(enabled), None, None);
            self.write_characteristic(&peripheral, control, &command).await?;
        } else {
            return Err(AppError::service_unavailable("Hue lamp has no power characteristic"));
        }

        self.update_state_after_write(lamp_id, |state| state.is_on = enabled).await
    }

    pub async fn set_brightness(&self, lamp_id: &str, brightness: u8) -> Result<HueLampState, AppError> {
        let brightness = brightness.clamp(1, 100);
        let raw = to_brightness(brightness);
        let (peripheral, characteristics) = self.connected_target(lamp_id).await?;

        if let Some(characteristic) = characteristics.brightness.as_ref() {
            self.write_characteristic(&peripheral, characteristic, &[raw]).await?;
        } else if let Some(control) = characteristics.control.as_ref() {
            let command = build_control_command(None, Some(raw), None);
            self.write_characteristic(&peripheral, control, &command).await?;
        } else {
            return Err(AppError::service_unavailable("Hue lamp has no brightness characteristic"));
        }

        self.update_state_after_write(lamp_id, |state| state.brightness = brightness).await
    }

    pub async fn set_temperature(&self, lamp_id: &str, temperature: u8) -> Result<HueLampState, AppError> {
        let temperature = temperature.clamp(0, 100);
        let raw = to_temperature(temperature);
        let (peripheral, characteristics) = self.connected_target(lamp_id).await?;

        if let Some(characteristic) = characteristics.temperature.as_ref() {
            self.write_characteristic(&peripheral, characteristic, &[raw, 0x01]).await?;
        } else if let Some(control) = characteristics.control.as_ref() {
            let command = build_control_command(None, None, Some(raw));
            self.write_characteristic(&peripheral, control, &command).await?;
        } else {
            return Err(AppError::service_unavailable("Hue lamp does not support color temperature"));
        }

        {
            let mut lamps = self.inner.lamps.write().await;
            if let Some(lamp) = lamps.get_mut(lamp_id) {
                lamp.config.last_temperature = Some(temperature);
            }
        }
        self.persist_state().await?;

        self.update_state_after_write(lamp_id, |state| state.temperature = Some(temperature)).await
    }

    pub async fn set_lamp_state(
        &self,
        lamp_id: &str,
        is_on: bool,
        brightness: Option<u8>,
    ) -> Result<HueLampState, AppError> {
        let (peripheral, characteristics) = self.connected_target(lamp_id).await?;
        if let Some(control) = characteristics.control.as_ref() {
            let command = build_control_command(
                Some(is_on),
                brightness.map(|value| to_brightness(value.clamp(1, 100))),
                None,
            );
            self.write_characteristic(&peripheral, control, &command).await?;
            self.update_state_after_write(lamp_id, |state| {
                state.is_on = is_on;
                if let Some(brightness) = brightness {
                    state.brightness = brightness.clamp(1, 100);
                }
            })
            .await
        } else {
            let _ = self.set_power(lamp_id, is_on).await?;
            if let Some(brightness) = brightness {
                self.set_brightness(lamp_id, brightness).await
            } else {
                self.current_state(lamp_id).await
            }
        }
    }

    pub async fn rename_lamp(&self, lamp_id: &str, name: &str) -> Result<bool, AppError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AppError::http(axum::http::StatusCode::BAD_REQUEST, "Lamp name cannot be empty"));
        }

        let lamp_key = self
            .resolve_lamp_key(lamp_id)
            .await
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"))?;

        let (peripheral, device_name) = {
            let mut lamps = self.inner.lamps.write().await;
            let Some(lamp) = lamps.get_mut(&lamp_key) else {
                return Err(AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"));
            };
            lamp.config.name = trimmed.to_string();
            (lamp.peripheral.clone(), lamp.characteristics.device_name.clone())
        };
        self.persist_state().await?;

        if let (Some(peripheral), Some(device_name)) = (peripheral, device_name) {
            let _ = self.write_characteristic(&peripheral, &device_name, trimmed.as_bytes()).await;
        }

        Ok(true)
    }

    pub async fn blacklist_lamp(&self, lamp_id: &str) -> Result<bool, AppError> {
        let lamp_key = match self.resolve_lamp_key(lamp_id).await {
            Some(key) => key,
            None => return Ok(false),
        };

        let (lamp_id, address, peripheral, notification_task) = {
            let mut lamps = self.inner.lamps.write().await;
            let Some(lamp) = lamps.remove(&lamp_key) else {
                return Ok(false);
            };
            (
                lamp.config.id,
                lamp.config.address,
                lamp.peripheral,
                lamp.notification_task,
            )
        };

        if let Some(task) = notification_task {
            task.abort();
        }

        if let Some(peripheral) = peripheral {
            let _guard = self.inner.ble_lock.lock().await;
            if matches!(peripheral.is_connected().await, Ok(true)) {
                let _ = timeout(IO_TIMEOUT, peripheral.disconnect()).await;
            }
        }

        self.inner
            .blacklisted_addresses
            .write()
            .await
            .extend([
                normalize_identity(&lamp_id),
                normalize_identity(&address),
            ]);
        self.persist_state().await?;
        Ok(true)
    }

    async fn initialize_runtime(&self) {
        let manager = match BleManager::new().await {
            Ok(manager) => manager,
            Err(error) => {
                warn!(error = %error, "Failed to initialize Bluetooth manager");
                *self.inner.availability_message.write().await =
                    Some(format!("Bluetooth unavailable: {error}"));
                return;
            }
        };

        let adapter = match manager.adapters().await {
            Ok(mut adapters) => adapters.drain(..).next(),
            Err(error) => {
                warn!(error = %error, "Failed to list Bluetooth adapters");
                *self.inner.availability_message.write().await =
                    Some(format!("Bluetooth adapter unavailable: {error}"));
                return;
            }
        };

        let Some(adapter) = adapter else {
            *self.inner.availability_message.write().await =
                Some("No Bluetooth adapter detected".to_string());
            return;
        };

        info!("Hue Bluetooth manager initialized");
        *self.inner.adapter.write().await = Some(adapter);
        *self.inner.availability_message.write().await = None;
        self.start_background_tasks().await;
    }

    async fn start_background_tasks(&self) {
        if let Some(adapter) = self.inner.adapter.read().await.clone() {
            let event_manager = self.clone();
            let event_task = tokio::spawn(async move {
                event_manager.run_adapter_events(adapter).await;
            });
            *self.inner.event_task.lock().await = Some(event_task);
        }

        let scan_manager = self.clone();
        let scan_task = tokio::spawn(async move {
            loop {
                if scan_manager.inner.shutting_down.load(Ordering::SeqCst) {
                    break;
                }
                if let Err(error) = scan_manager.perform_scan(SCAN_DURATION).await {
                    debug!(error = %error, "Hue periodic scan failed");
                }
                sleep(SCAN_INTERVAL).await;
            }
        });
        *self.inner.scan_task.lock().await = Some(scan_task);

        let poll_manager = self.clone();
        let poll_task = tokio::spawn(async move {
            loop {
                if poll_manager.inner.shutting_down.load(Ordering::SeqCst) {
                    break;
                }
                sleep(POLL_INTERVAL).await;
                if poll_manager.inner.shutting_down.load(Ordering::SeqCst) {
                    break;
                }
                poll_manager.poll_connected_lamps().await;
            }
        });
        *self.inner.poll_task.lock().await = Some(poll_task);
    }

    async fn perform_scan(&self, duration: Duration) -> Result<(), AppError> {
        let Some(adapter) = self.inner.adapter.read().await.clone() else {
            return Ok(());
        };

        let guard = self.inner.ble_lock.lock().await;
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|error| AppError::service_unavailable(format!("Bluetooth scan failed: {error}")))?;

        sleep(duration).await;

        let _ = adapter.stop_scan().await;

        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|error| AppError::service_unavailable(format!("Bluetooth scan failed: {error}")))?;

        drop(guard);

        let mut discovered_identities = HashSet::new();
        let mut connect_queue = Vec::new();

        for peripheral in peripherals {
            let Some(properties) = peripheral.properties().await.ok().flatten() else {
                continue;
            };
            if !is_hue_lamp(&properties) {
                continue;
            }

            let stable_id = stable_lamp_id(&peripheral, &properties);
            let display_address = lamp_display_address(&peripheral, &properties);
            if self
                .inner
                .blacklisted_addresses
                .read()
                .await
                .iter()
                .any(|entry| entry == &stable_id || entry == &normalize_identity(&display_address))
            {
                continue;
            }

            discovered_identities.insert(stable_id.clone());
            let name = properties
                .local_name
                .clone()
                .or(properties.advertisement_name.clone())
                .unwrap_or_else(|| fallback_lamp_name(&display_address));

            let (lamp_id, should_connect) = {
                let mut lamps = self.inner.lamps.write().await;
                let matched_key = lamps
                    .iter()
                    .find(|(_, lamp)| lamp_matches_scan(lamp, &stable_id, &display_address, &name))
                    .map(|(id, _)| id.clone());

                if let Some(existing_key) = matched_key {
                    let final_key = stable_id.clone();
                    let mut lamp = lamps.remove(&existing_key).expect("matched lamp should exist");
                    lamp.config.id = stable_id.clone();
                    lamp.config.address = display_address.clone();
                    lamp.peripheral = Some(peripheral.clone());
                    lamp.reachable = true;
                    lamp.last_seen = Some(Utc::now());
                    let should_connect = !lamp.connected && !lamp.connecting;
                    lamps.insert(final_key.clone(), lamp);
                    (final_key, should_connect)
                } else {
                    let config = StoredLampConfig {
                        id: stable_id.clone(),
                        name,
                        address: display_address.clone(),
                        model: None,
                        has_connected_once: false,
                        temperature_min: None,
                        temperature_max: None,
                        last_temperature: None,
                    };
                    let id = config.id.clone();
                    lamps.insert(
                        id.clone(),
                        LampRuntime {
                            config,
                            state: RuntimeLampState {
                                is_on: false,
                                brightness: 100,
                                temperature: None,
                                temperature_min: None,
                                temperature_max: None,
                            },
                            info: LampInfo {
                                manufacturer: "Philips Hue".to_string(),
                                firmware: None,
                                model: None,
                            },
                            connected: false,
                            connecting: false,
                            reachable: true,
                            last_seen: Some(Utc::now()),
                            peripheral: Some(peripheral.clone()),
                            characteristics: HueCharacteristics::default(),
                            connection_failures: 0,
                            notification_task: None,
                        },
                    );
                    (id, true)
                }
            };

            if should_connect {
                connect_queue.push(lamp_id);
            }
        }

        {
            let mut lamps = self.inner.lamps.write().await;
            for lamp in lamps.values_mut() {
                let known = normalize_identity(&lamp.config.id);
                if !discovered_identities.contains(&known) && !lamp.connected {
                    lamp.reachable = false;
                    lamp.peripheral = None;
                }
            }
        }

        self.persist_state().await?;

        for lamp_id in connect_queue {
            let _ = self.connect_lamp(&lamp_id).await;
        }

        Ok(())
    }

    async fn poll_connected_lamps(&self) {
        let lamp_ids = {
            let lamps = self.inner.lamps.read().await;
            lamps
                .iter()
                .filter_map(|(id, lamp)| lamp.connected.then_some(id.clone()))
                .collect::<Vec<_>>()
        };

        for lamp_id in lamp_ids {
            if self.refresh_lamp_state(&lamp_id).await.is_err() {
                self.mark_runtime_disconnected(&lamp_id).await;
            }
        }
    }

    async fn run_adapter_events(&self, adapter: Adapter) {
        let mut events = match adapter.events().await {
            Ok(stream) => stream,
            Err(error) => {
                warn!(error = %error, "Failed to subscribe to Bluetooth adapter events");
                return;
            }
        };

        while let Some(event) = events.next().await {
            if self.inner.shutting_down.load(Ordering::SeqCst) {
                break;
            }
            self.handle_adapter_event(event).await;
        }
    }

    async fn handle_adapter_event(&self, event: CentralEvent) {
        match event {
            CentralEvent::DeviceConnected(id) => {
                let identity = normalize_identity(&id.to_string());
                self.mark_runtime_connected(&identity).await;
            }
            CentralEvent::DeviceDisconnected(id) => {
                let identity = normalize_identity(&id.to_string());
                self.mark_runtime_disconnected(&identity).await;
            }
            CentralEvent::DeviceUpdated(id)
            | CentralEvent::DeviceDiscovered(id)
            | CentralEvent::DeviceServicesModified(id) => {
                let identity = normalize_identity(&id.to_string());
                self.mark_runtime_seen(&identity).await;
            }
            CentralEvent::ManufacturerDataAdvertisement { id, .. }
            | CentralEvent::ServiceDataAdvertisement { id, .. }
            | CentralEvent::ServicesAdvertisement { id, .. }
            | CentralEvent::RssiUpdate { id, .. } => {
                let identity = normalize_identity(&id.to_string());
                self.mark_runtime_seen(&identity).await;
            }
            CentralEvent::StateUpdate(_) => {}
        }
    }

    async fn establish_connection(
        &self,
        lamp_id: &str,
        peripheral: Peripheral,
    ) -> Result<ConnectionReady, AppError> {
        {
            let _guard = self.inner.ble_lock.lock().await;
            let already_connected = peripheral
                .is_connected()
                .await
                .map_err(|error| AppError::service_unavailable(format!("Bluetooth error: {error}")))?;

            if !already_connected {
                timeout(CONNECT_TIMEOUT, peripheral.connect())
                    .await
                    .map_err(|_| AppError::service_unavailable("Hue lamp connection timed out"))?
                    .map_err(|error| AppError::service_unavailable(format!("Bluetooth error: {error}")))?;
            }

            timeout(IO_TIMEOUT, peripheral.discover_services())
                .await
                .map_err(|_| AppError::service_unavailable("Hue lamp service discovery timed out"))?
                .map_err(|error| AppError::service_unavailable(format!("Bluetooth error: {error}")))?;
        }

        let characteristics = HueCharacteristics::from_peripheral(&peripheral);
        let info = self.read_device_info(&peripheral, &characteristics).await;
        let state = self.read_state(&peripheral, &characteristics).await?;
        self.start_notification_listener(lamp_id, &peripheral, &characteristics)
            .await;

        Ok(ConnectionReady {
            peripheral,
            characteristics,
            info,
            state,
        })
    }

    async fn finish_successful_connection(
        &self,
        lamp_id: &str,
        ready: ConnectionReady,
    ) -> Result<(), AppError> {
        {
            let mut lamps = self.inner.lamps.write().await;
            let Some(lamp) = lamps.get_mut(lamp_id) else {
                return Ok(());
            };

            lamp.connected = true;
            lamp.connecting = false;
            lamp.reachable = true;
            lamp.last_seen = Some(Utc::now());
            lamp.peripheral = Some(ready.peripheral);
            lamp.characteristics = ready.characteristics;
            lamp.info = ready.info.clone();
            lamp.state = ready.state.clone();
            lamp.connection_failures = 0;
            lamp.config.has_connected_once = true;
            if lamp.config.model.is_none() {
                lamp.config.model = ready.info.model.clone();
            }
            if lamp.state.temperature_min.is_some() {
                lamp.config.temperature_min = lamp.state.temperature_min;
            }
            if lamp.state.temperature_max.is_some() {
                lamp.config.temperature_max = lamp.state.temperature_max;
            }
        }
        self.persist_state().await?;
        Ok(())
    }

    async fn mark_connect_failure(&self, lamp_id: &str) -> Result<(), AppError> {
        let should_blacklist = {
            let mut lamps = self.inner.lamps.write().await;
            let Some(lamp) = lamps.get_mut(lamp_id) else {
                return Ok(());
            };
            lamp.connected = false;
            lamp.connecting = false;
            lamp.reachable = false;
            lamp.characteristics = HueCharacteristics::default();
            lamp.connection_failures = lamp.connection_failures.saturating_add(1);
            !lamp.config.has_connected_once && lamp.connection_failures >= MAX_NEW_LAMP_FAILURES
        };

        if should_blacklist {
            let _ = self.blacklist_lamp(lamp_id).await?;
        }

        Ok(())
    }

    async fn mark_runtime_connected(&self, lamp_id: &str) {
        let Some(lamp_key) = self.resolve_lamp_key(lamp_id).await else {
            return;
        };

        let mut lamps = self.inner.lamps.write().await;
        if let Some(lamp) = lamps.get_mut(&lamp_key) {
            lamp.connected = true;
            lamp.connecting = false;
            lamp.reachable = true;
            lamp.last_seen = Some(Utc::now());
        }
    }

    async fn mark_runtime_disconnected(&self, lamp_id: &str) {
        let Some(lamp_key) = self.resolve_lamp_key(lamp_id).await else {
            return;
        };

        let mut lamps = self.inner.lamps.write().await;
        if let Some(lamp) = lamps.get_mut(&lamp_key) {
            lamp.connected = false;
            lamp.connecting = false;
            lamp.reachable = false;
            lamp.last_seen = Some(Utc::now());
            lamp.characteristics = HueCharacteristics::default();
            if let Some(task) = lamp.notification_task.take() {
                task.abort();
            }
        }
    }

    async fn mark_runtime_seen(&self, lamp_id: &str) {
        let Some(lamp_key) = self.resolve_lamp_key(lamp_id).await else {
            return;
        };

        let mut lamps = self.inner.lamps.write().await;
        if let Some(lamp) = lamps.get_mut(&lamp_key) {
            lamp.reachable = true;
            lamp.last_seen = Some(Utc::now());
        }
    }

    async fn connected_target(
        &self,
        lamp_id: &str,
    ) -> Result<(Peripheral, HueCharacteristics), AppError> {
        let lamp_key = self
            .resolve_lamp_key(lamp_id)
            .await
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"))?;

        let (peripheral, characteristics, connected) = {
            let lamps = self.inner.lamps.read().await;
            let Some(lamp) = lamps.get(&lamp_key) else {
                return Err(AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"));
            };
            (
                lamp.peripheral.clone(),
                lamp.characteristics.clone(),
                lamp.connected,
            )
        };

        if !connected {
            return Err(AppError::service_unavailable("Hue lamp is not connected"));
        }

        let Some(peripheral) = peripheral else {
            return Err(AppError::service_unavailable("Hue lamp is not available"));
        };

        Ok((peripheral, characteristics))
    }

    async fn write_characteristic(
        &self,
        peripheral: &Peripheral,
        characteristic: &Characteristic,
        payload: &[u8],
    ) -> Result<(), AppError> {
        let _guard = self.inner.ble_lock.lock().await;
        timeout(
            IO_TIMEOUT,
            peripheral.write(characteristic, payload, WriteType::WithoutResponse),
        )
        .await
        .map_err(|_| AppError::service_unavailable("Hue lamp write timed out"))?
        .map_err(|error| AppError::service_unavailable(format!("Bluetooth error: {error}")))
    }

    async fn update_state_after_write<F>(&self, lamp_id: &str, update: F) -> Result<HueLampState, AppError>
    where
        F: FnOnce(&mut RuntimeLampState),
    {
        let lamp_key = self
            .resolve_lamp_key(lamp_id)
            .await
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"))?;

        {
            let mut lamps = self.inner.lamps.write().await;
            let Some(lamp) = lamps.get_mut(&lamp_key) else {
                return Err(AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"));
            };
            update(&mut lamp.state);
            lamp.connected = true;
            lamp.reachable = true;
        }
        self.current_state(&lamp_key).await
    }

    async fn current_state(&self, lamp_id: &str) -> Result<HueLampState, AppError> {
        let lamp_key = self
            .resolve_lamp_key(lamp_id)
            .await
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"))?;

        let lamps = self.inner.lamps.read().await;
        let Some(lamp) = lamps.get(&lamp_key) else {
            return Err(AppError::http(axum::http::StatusCode::NOT_FOUND, "Hue lamp not found"));
        };
        Ok(HueLampState {
            is_on: lamp.state.is_on,
            brightness: lamp.state.brightness,
            temperature: lamp.state.temperature,
            temperature_min: lamp.state.temperature_min,
            temperature_max: lamp.state.temperature_max,
        })
    }

    async fn read_state(
        &self,
        peripheral: &Peripheral,
        characteristics: &HueCharacteristics,
    ) -> Result<RuntimeLampState, AppError> {
        let _guard = self.inner.ble_lock.lock().await;
        let mut state = RuntimeLampState {
            is_on: false,
            brightness: 100,
            temperature: None,
            temperature_min: Some(0),
            temperature_max: Some(100),
        };

        if let Some(power) = characteristics.power.as_ref() {
            let bytes = timeout(IO_TIMEOUT, peripheral.read(power))
                .await
                .map_err(|_| AppError::service_unavailable("Hue lamp read timed out"))?
                .map_err(|error| AppError::service_unavailable(format!("Bluetooth error: {error}")))?;
            state.is_on = bytes.first().copied().unwrap_or_default() == 0x01;
        }

        if let Some(brightness) = characteristics.brightness.as_ref() {
            let bytes = timeout(IO_TIMEOUT, peripheral.read(brightness))
                .await
                .map_err(|_| AppError::service_unavailable("Hue lamp read timed out"))?
                .map_err(|error| AppError::service_unavailable(format!("Bluetooth error: {error}")))?;
            if let Some(raw) = bytes.first().copied() {
                state.brightness = parse_brightness(raw);
            }
        }

        if let Some(temperature) = characteristics.temperature.as_ref() {
            let bytes = timeout(IO_TIMEOUT, peripheral.read(temperature))
                .await
                .map_err(|_| AppError::service_unavailable("Hue lamp read timed out"))?
                .map_err(|error| AppError::service_unavailable(format!("Bluetooth error: {error}")))?;
            if let Some(raw) = bytes.first().copied() {
                state.temperature = Some(parse_temperature(raw));
            }
        } else {
            state.temperature = None;
            state.temperature_min = None;
            state.temperature_max = None;
        }

        Ok(state)
    }

    async fn read_device_info(
        &self,
        peripheral: &Peripheral,
        characteristics: &HueCharacteristics,
    ) -> LampInfo {
        let _guard = self.inner.ble_lock.lock().await;
        LampInfo {
            manufacturer: self
                .read_optional_string(peripheral, characteristics.manufacturer.as_ref())
                .await
                .unwrap_or_else(|| "Philips Hue".to_string()),
            firmware: self
                .read_optional_string(peripheral, characteristics.firmware.as_ref())
                .await,
            model: self.read_optional_string(peripheral, characteristics.model.as_ref()).await,
        }
    }

    async fn read_optional_string(
        &self,
        peripheral: &Peripheral,
        characteristic: Option<&Characteristic>,
    ) -> Option<String> {
        let characteristic = characteristic?;
        let value = timeout(IO_TIMEOUT, peripheral.read(characteristic)).await.ok()?.ok()?;
        let parsed = String::from_utf8(value).ok()?;
        let trimmed = parsed.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    async fn start_notification_listener(
        &self,
        lamp_id: &str,
        peripheral: &Peripheral,
        characteristics: &HueCharacteristics,
    ) {
        let mut subscribable = Vec::new();
        for characteristic in [
            characteristics.power.clone(),
            characteristics.brightness.clone(),
            characteristics.temperature.clone(),
            characteristics.control.clone(),
        ]
        .into_iter()
        .flatten()
        {
            if characteristic
                .properties
                .intersects(CharPropFlags::NOTIFY | CharPropFlags::INDICATE)
            {
                subscribable.push(characteristic);
            }
        }

        if subscribable.is_empty() {
            return;
        }

        let stream = match peripheral.notifications().await {
            Ok(stream) => stream,
            Err(error) => {
                debug!(lamp_id, error = %error, "Hue notifications unavailable");
                return;
            }
        };

        for characteristic in &subscribable {
            let _ = peripheral.subscribe(characteristic).await;
        }

        let manager = self.clone();
        let lamp_id_owned = lamp_id.to_string();
        let task = tokio::spawn(async move {
            let mut stream = stream;
            while let Some(notification) = stream.next().await {
                manager.handle_notification(&lamp_id_owned, notification).await;
            }
        });

        let mut lamps = self.inner.lamps.write().await;
        if let Some(lamp) = lamps.get_mut(lamp_id) {
            if let Some(previous) = lamp.notification_task.replace(task) {
                previous.abort();
            }
        }
    }

    async fn handle_notification(&self, lamp_id: &str, notification: ValueNotification) {
        let mut refresh_full_state = false;
        {
            let mut lamps = self.inner.lamps.write().await;
            let Some(lamp) = lamps.get_mut(lamp_id) else {
                return;
            };
            let uuid = normalize_uuid(notification.uuid);
            let data = notification.value;
            if uuid == normalize_uuid_str(POWER_UUID) {
                lamp.state.is_on = data.first().copied().unwrap_or_default() == 0x01;
            } else if uuid == normalize_uuid_str(BRIGHTNESS_UUID) {
                if let Some(raw) = data.first().copied() {
                    lamp.state.brightness = parse_brightness(raw);
                }
            } else if uuid == normalize_uuid_str(TEMPERATURE_UUID) {
                if let Some(raw) = data.first().copied() {
                    lamp.state.temperature = Some(parse_temperature(raw));
                }
            } else if uuid == normalize_uuid_str(CONTROL_UUID) {
                refresh_full_state = true;
            }
            lamp.last_seen = Some(Utc::now());
            lamp.connected = true;
            lamp.reachable = true;
        }

        if refresh_full_state {
            let _ = self.refresh_lamp_state(lamp_id).await;
        }
    }

    async fn persist_state(&self) -> Result<(), AppError> {
        let lamp_configs = {
            let lamps = self.inner.lamps.read().await;
            lamps.values().map(|lamp| lamp.config.clone()).collect::<Vec<_>>()
        };
        let blacklist = {
            let blacklist = self.inner.blacklisted_addresses.read().await;
            blacklist.iter().cloned().collect::<Vec<_>>()
        };

        self.inner.store.save_lamps(&lamp_configs)?;
        self.inner.store.save_blacklist(&blacklist)?;
        Ok(())
    }

    async fn resolve_lamp_key(&self, lamp_id: &str) -> Option<String> {
        let lamps = self.inner.lamps.read().await;
        if lamps.contains_key(lamp_id) {
            return Some(lamp_id.to_string());
        }

        let requested = normalize_identity(lamp_id);
        lamps.iter().find_map(|(key, lamp)| {
            let key_matches = normalize_identity(key) == requested;
            let id_matches = normalize_identity(&lamp.config.id) == requested;
            let address_matches = normalize_identity(&lamp.config.address) == requested;
            (key_matches || id_matches || address_matches).then(|| key.clone())
        })
    }
}

impl HueCharacteristics {
    fn from_peripheral(peripheral: &Peripheral) -> Self {
        let mut characteristics = Self::default();
        for characteristic in peripheral.characteristics() {
            let uuid = normalize_uuid(characteristic.uuid);
            if uuid == normalize_uuid_str(POWER_UUID) {
                characteristics.power = Some(characteristic.clone());
            } else if uuid == normalize_uuid_str(BRIGHTNESS_UUID) {
                characteristics.brightness = Some(characteristic.clone());
            } else if uuid == normalize_uuid_str(TEMPERATURE_UUID) {
                characteristics.temperature = Some(characteristic.clone());
            } else if uuid == normalize_uuid_str(CONTROL_UUID) {
                characteristics.control = Some(characteristic.clone());
            } else if uuid == normalize_uuid_str(MODEL_UUID) {
                characteristics.model = Some(characteristic.clone());
            } else if uuid == normalize_uuid_str(FIRMWARE_UUID) {
                characteristics.firmware = Some(characteristic.clone());
            } else if uuid == normalize_uuid_str(MANUFACTURER_UUID) {
                characteristics.manufacturer = Some(characteristic.clone());
            } else if uuid == normalize_uuid_str(DEVICE_NAME_UUID) {
                characteristics.device_name = Some(characteristic.clone());
            }
        }
        characteristics
    }
}

impl HueStore {
    fn load_lamps(&self) -> Vec<StoredLampConfig> {
        dedupe_stored_lamps(
            self.read_json::<Vec<StoredLampConfig>>(&self.lamps_path)
                .unwrap_or_default(),
        )
    }

    fn load_blacklist(&self) -> HashSet<String> {
        self.read_json::<Vec<String>>(&self.blacklist_path)
            .unwrap_or_default()
            .into_iter()
            .map(|address| normalize_address(&address))
            .collect()
    }

    fn save_lamps(&self, lamps: &[StoredLampConfig]) -> Result<(), AppError> {
        self.write_json(&self.lamps_path, lamps)
    }

    fn save_blacklist(&self, blacklist: &[String]) -> Result<(), AppError> {
        self.write_json(&self.blacklist_path, blacklist)
    }

    fn read_json<T>(&self, path: &Path) -> Option<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let content = fs::read_to_string(path).ok()?;
        match serde_json::from_str(&content) {
            Ok(value) => Some(value),
            Err(error) => {
                warn!(path = %path.display(), error = %error, "Failed to parse Hue persistence file");
                None
            }
        }
    }

    fn write_json<T>(&self, path: &Path, value: &T) -> Result<(), AppError>
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
}

fn to_view(lamp: &LampRuntime) -> HueLampView {
    HueLampView {
        id: lamp.config.id.clone(),
        name: lamp.config.name.clone(),
        address: lamp.config.address.clone(),
        model: lamp.info.model.clone().or_else(|| lamp.config.model.clone()),
        manufacturer: lamp.info.manufacturer.clone(),
        firmware: lamp.info.firmware.clone(),
        connected: lamp.connected,
        connecting: lamp.connecting,
        reachable: lamp.reachable,
        state: HueLampState {
            is_on: lamp.state.is_on,
            brightness: lamp.state.brightness,
            temperature: lamp.state.temperature,
            temperature_min: lamp.state.temperature_min,
            temperature_max: lamp.state.temperature_max,
        },
        last_seen: lamp.last_seen.map(|value| value.to_rfc3339()),
    }
}

fn is_hue_lamp(properties: &PeripheralProperties) -> bool {
    if properties.services.iter().any(|uuid| {
        let uuid = normalize_uuid(*uuid);
        uuid == normalize_uuid_str(LIGHT_CONTROL_SERVICE) || uuid == normalize_uuid_str(CONFIG_SERVICE_UUID)
    }) {
        return true;
    }

    if properties
        .manufacturer_data
        .keys()
        .any(|id| *id == PHILIPS_MANUFACTURER_ID || *id == SIGNIFY_MANUFACTURER_ID)
    {
        return true;
    }

    let name = properties
        .local_name
        .as_deref()
        .or(properties.advertisement_name.as_deref())
        .unwrap_or_default()
        .to_ascii_lowercase();

    ["hue", "philips", "signify", "lwa", "lwv", "ltg", "lct", "lwb", "lca"]
        .iter()
        .any(|needle| name.contains(needle))
}

fn parse_brightness(raw_value: u8) -> u8 {
    let clamped = raw_value.clamp(1, 254);
    ((u16::from(clamped) * 100) / 254).max(1) as u8
}

fn to_brightness(percentage: u8) -> u8 {
    let clamped = percentage.clamp(1, 100);
    (((u16::from(clamped) * 254) + 50) / 100) as u8
}

fn parse_temperature(raw_value: u8) -> u8 {
    let clamped = raw_value.clamp(1, 244);
    (((244_u16.saturating_sub(u16::from(clamped))) * 100) / 243) as u8
}

fn to_temperature(percentage: u8) -> u8 {
    let clamped = percentage.clamp(0, 100);
    (244_u16.saturating_sub((u16::from(clamped) * 243) / 100)) as u8
}

fn build_control_command(power: Option<bool>, brightness: Option<u8>, temperature: Option<u8>) -> Vec<u8> {
    let mut commands = Vec::new();
    if let Some(power) = power {
        commands.extend_from_slice(&[0x01, 0x01, u8::from(power)]);
    }
    if let Some(brightness) = brightness {
        commands.extend_from_slice(&[0x02, 0x01, brightness.clamp(1, 254)]);
    }
    if let Some(temperature) = temperature {
        commands.extend_from_slice(&[0x03, 0x02, temperature.clamp(1, 244), 0x01]);
    }
    commands
}

fn normalize_uuid(uuid: Uuid) -> String {
    uuid.as_hyphenated().to_string().replace('-', "").to_ascii_lowercase()
}

fn normalize_uuid_str(uuid: &str) -> String {
    uuid.replace('-', "").to_ascii_lowercase()
}

fn normalize_address(address: &str) -> String {
    address.trim().to_ascii_lowercase()
}

fn normalize_identity(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect()
}

fn is_zero_address(address: &str) -> bool {
    normalize_address(address) == ZERO_BLE_ADDRESS
        || normalize_identity(address) == normalize_identity(ZERO_BLE_ADDRESS)
}

fn stable_lamp_id(peripheral: &Peripheral, properties: &PeripheralProperties) -> String {
    let address = normalize_address(&properties.address.to_string());
    if is_zero_address(&address) {
        normalize_identity(&peripheral.id().to_string())
    } else {
        normalize_identity(&address)
    }
}

fn lamp_display_address(peripheral: &Peripheral, properties: &PeripheralProperties) -> String {
    let address = normalize_address(&properties.address.to_string());
    if is_zero_address(&address) {
        peripheral.id().to_string()
    } else {
        address
    }
}

fn lamp_matches_scan(
    lamp: &LampRuntime,
    stable_id: &str,
    display_address: &str,
    discovered_name: &str,
) -> bool {
    let lamp_id = normalize_identity(&lamp.config.id);
    let lamp_address = normalize_identity(&lamp.config.address);
    lamp_id == stable_id
        || lamp_address == stable_id
        || lamp_address == normalize_identity(display_address)
        || ((lamp_id == ZERO_BLE_ADDRESS || lamp_address == ZERO_BLE_ADDRESS)
            && lamp.config.name == discovered_name)
}

fn dedupe_stored_lamps(configs: Vec<StoredLampConfig>) -> Vec<StoredLampConfig> {
    let mut deduped: HashMap<String, StoredLampConfig> = HashMap::new();

    for config in configs {
        let canonical = canonical_stored_identity(&config);
        let normalized = normalize_stored_config(config, &canonical);
        match deduped.get(&canonical) {
            Some(existing) if !prefer_stored_config(&normalized, existing) => {}
            _ => {
                deduped.insert(canonical, normalized);
            }
        }
    }

    deduped.into_values().collect()
}

fn canonical_stored_identity(config: &StoredLampConfig) -> String {
    let normalized_id = normalize_identity(&config.id);
    let normalized_address = normalize_identity(&config.address);

    if !normalized_id.is_empty() && normalized_id != normalize_identity(ZERO_BLE_ADDRESS) {
        normalized_id
    } else {
        normalized_address
    }
}

fn normalize_stored_config(mut config: StoredLampConfig, canonical: &str) -> StoredLampConfig {
    config.id = canonical.to_string();

    let normalized_address = normalize_identity(&config.address);
    if normalized_address.is_empty() || normalized_address == normalize_identity(ZERO_BLE_ADDRESS) {
        config.address = canonical.to_string();
    }

    config
}

fn prefer_stored_config(candidate: &StoredLampConfig, current: &StoredLampConfig) -> bool {
    stored_config_score(candidate) > stored_config_score(current)
}

fn stored_config_score(config: &StoredLampConfig) -> usize {
    usize::from(config.has_connected_once) * 10
        + usize::from(config.model.is_some()) * 5
        + usize::from(config.temperature_min.is_some())
        + usize::from(config.temperature_max.is_some())
        + usize::from(config.last_temperature.is_some())
}

fn fallback_lamp_name(address: &str) -> String {
    let suffix = address.chars().rev().take(5).collect::<String>();
    format!("Hue Lamp {}", suffix.chars().rev().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::{build_control_command, parse_brightness, parse_temperature, to_brightness, to_temperature};

    #[test]
    fn brightness_conversion_round_trips_reasonably() {
        assert_eq!(parse_brightness(to_brightness(1)), 1);
        assert!(parse_brightness(to_brightness(50)) >= 49);
        assert_eq!(parse_brightness(to_brightness(100)), 100);
    }

    #[test]
    fn temperature_conversion_round_trips_reasonably() {
        assert_eq!(parse_temperature(to_temperature(0)), 0);
        assert!(parse_temperature(to_temperature(50)) <= 50);
        assert!(parse_temperature(to_temperature(100)) >= 99);
    }

    #[test]
    fn combined_control_command_contains_expected_segments() {
        assert_eq!(
            build_control_command(Some(true), Some(10), Some(20)),
            vec![0x01, 0x01, 0x01, 0x02, 0x01, 10, 0x03, 0x02, 20, 0x01]
        );
    }
}
