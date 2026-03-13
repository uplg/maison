use std::{collections::HashMap, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::AppError;

const HTTP_TIMEOUT_MS: u64 = 5_000;

#[derive(Debug, Clone, Deserialize)]
pub struct MerossDeviceConfig {
    pub name: String,
    pub ip: String,
    pub key: String,
    pub uuid: Option<String>,
    pub mac: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MerossDeviceListEntry {
    pub id: String,
    pub name: String,
    pub ip: String,
    #[serde(rename = "isOnline")]
    pub is_online: bool,
    #[serde(rename = "isOn")]
    pub is_on: bool,
    #[serde(rename = "lastPing")]
    pub last_ping: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MerossStatus {
    pub online: bool,
    pub on: bool,
    pub electricity: Option<MerossStatusElectricity>,
    pub hardware: Option<MerossHardware>,
    pub firmware: Option<MerossFirmware>,
    pub wifi: MerossWifi,
    #[serde(rename = "lastUpdate")]
    pub last_update: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MerossStatusElectricity {
    pub voltage: f64,
    pub current: f64,
    pub power: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MerossHardware {
    #[serde(rename = "type")]
    pub device_type: String,
    pub version: String,
    #[serde(rename = "chipType")]
    pub chip_type: String,
    pub uuid: String,
    pub mac: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MerossFirmware {
    pub version: String,
    #[serde(rename = "compileTime")]
    pub compile_time: String,
    #[serde(rename = "innerIp")]
    pub inner_ip: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MerossWifi {
    pub signal: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MerossElectricityFormatted {
    pub voltage: String,
    pub current: String,
    pub power: String,
    pub raw: MerossElectricityRaw,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerossElectricityRaw {
    pub channel: i32,
    pub current: i32,
    pub voltage: i32,
    pub power: i32,
    pub config: Option<MerossElectricityConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerossElectricityConfig {
    #[serde(rename = "voltageRatio")]
    pub voltage_ratio: i32,
    #[serde(rename = "electricityRatio")]
    pub electricity_ratio: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerossConsumptionEntry {
    pub date: String,
    pub time: i64,
    pub value: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MerossConsumptionSummary {
    pub days: usize,
    #[serde(rename = "totalWh")]
    pub total_wh: i32,
    #[serde(rename = "totalKwh")]
    pub total_kwh: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MerossToggleResponse {
    pub device: MerossDeviceRef,
    pub on: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MerossDndResponse {
    pub device: MerossDeviceRef,
    #[serde(rename = "dndMode")]
    pub dnd_mode: bool,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct MerossManager {
    client: reqwest::Client,
    devices: Arc<HashMap<String, MerossManagedDevice>>,
}

#[derive(Debug, Clone)]
struct MerossManagedDevice {
    config: MerossDeviceConfig,
    state: Arc<RwLock<MerossCachedState>>,
}

#[derive(Debug, Clone, Default)]
struct MerossCachedState {
    is_online: bool,
    is_on: bool,
    last_ping: i64,
}

#[derive(Debug, Serialize)]
pub struct MerossStats {
    pub total: usize,
    pub online: usize,
    pub offline: usize,
    pub devices: Vec<MerossStatsEntry>,
}

#[derive(Debug, Serialize)]
pub struct MerossStatsEntry {
    pub id: String,
    pub name: String,
    pub ip: String,
    #[serde(rename = "isOnline")]
    pub is_online: bool,
    #[serde(rename = "lastPing")]
    pub last_ping: i64,
}

#[derive(Debug, Deserialize)]
struct MerossPacket {
    header: MerossHeader,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct MerossHeader {
    method: String,
}

#[derive(Debug, Serialize)]
struct MerossRequestPacket<'a> {
    header: MerossRequestHeader<'a>,
    payload: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct MerossRequestHeader<'a> {
    from: String,
    #[serde(rename = "messageId")]
    message_id: String,
    method: &'a str,
    namespace: &'a str,
    #[serde(rename = "payloadVersion")]
    payload_version: i32,
    sign: String,
    timestamp: i64,
    #[serde(rename = "timestampMs")]
    timestamp_ms: i64,
}

#[derive(Debug, Deserialize)]
struct MerossSystemAllPayload {
    all: MerossSystemAll,
}

#[derive(Debug, Deserialize)]
struct MerossSystemAll {
    system: MerossSystem,
    control: Option<MerossDigest>,
    digest: Option<MerossDigest>,
}

#[derive(Debug, Deserialize)]
struct MerossSystem {
    hardware: MerossSystemHardware,
    firmware: MerossSystemFirmware,
}

#[derive(Debug, Deserialize)]
struct MerossSystemHardware {
    #[serde(rename = "type")]
    device_type: String,
    version: String,
    #[serde(rename = "chipType")]
    chip_type: String,
    uuid: String,
    #[serde(rename = "macAddress")]
    mac_address: String,
}

#[derive(Debug, Deserialize)]
struct MerossSystemFirmware {
    version: String,
    #[serde(rename = "compileTime")]
    compile_time: String,
    #[serde(rename = "innerIp")]
    inner_ip: String,
}

#[derive(Debug, Deserialize)]
struct MerossDigest {
    toggle: Option<MerossToggle>,
    togglex: Option<Vec<MerossToggleX>>,
}

#[derive(Debug, Deserialize)]
struct MerossToggle {
    onoff: i32,
}

#[derive(Debug, Deserialize)]
struct MerossToggleX {
    onoff: i32,
}

#[derive(Debug, Deserialize)]
struct MerossRuntimePayload {
    runtime: MerossRuntime,
}

#[derive(Debug, Deserialize)]
struct MerossRuntime {
    signal: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct MerossElectricityPayload {
    electricity: MerossElectricityRaw,
}

#[derive(Debug, Deserialize)]
struct MerossConsumptionPayload {
    consumptionx: Option<Vec<MerossConsumptionEntry>>,
}

impl MerossManager {
    pub fn new(config_path: &std::path::Path) -> Result<Self, AppError> {
        let content = std::fs::read_to_string(config_path)?;
        let configs = serde_json::from_str::<Vec<MerossDeviceConfig>>(&content)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(HTTP_TIMEOUT_MS))
            .build()?;

        let devices = configs
            .into_iter()
            .map(|config| {
                let id = config.ip.clone();
                (
                    id,
                    MerossManagedDevice {
                        config,
                        state: Arc::new(RwLock::new(MerossCachedState::default())),
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        Ok(Self {
            client,
            devices: Arc::new(devices),
        })
    }

    pub async fn list_devices(&self) -> Vec<MerossDeviceListEntry> {
        let mut entries = Vec::with_capacity(self.devices.len());
        for (id, device) in self.devices.iter() {
            let snapshot = self.snapshot_device(device).await;
            entries.push(MerossDeviceListEntry {
                id: id.clone(),
                name: device.config.name.clone(),
                ip: device.config.ip.clone(),
                is_online: snapshot.is_online,
                is_on: snapshot.is_on,
                last_ping: snapshot.last_ping,
            });
        }
        entries
    }

    pub async fn get_stats(&self) -> MerossStats {
        let devices = self.list_devices().await;
        let online = devices.iter().filter(|device| device.is_online).count();
        MerossStats {
            total: devices.len(),
            online,
            offline: devices.len().saturating_sub(online),
            devices: devices
                .into_iter()
                .map(|device| MerossStatsEntry {
                    id: device.id,
                    name: device.name,
                    ip: device.ip,
                    is_online: device.is_online,
                    last_ping: device.last_ping,
                })
                .collect(),
        }
    }

    pub async fn get_status(&self, device_id: &str) -> Result<(MerossDeviceRef, MerossStatus), AppError> {
        let device = self.get_device(device_id)?;
        let system = self.get_system_all(device).await?;
        let electricity = self.get_electricity_raw(device).await.ok().map(|raw| MerossStatusElectricity {
            voltage: f64::from(raw.voltage) / 10.0,
            current: f64::from(raw.current) / 1000.0,
            power: f64::from(raw.power) / 1000.0,
        });
        let signal = self.get_runtime_signal(device).await.ok().flatten();

        let is_on = extract_on_state(&system.all);
        let now = now_millis();
        {
            let mut state = device.state.write().await;
            state.is_online = true;
            state.is_on = is_on;
            state.last_ping = now;
        }

        let status = MerossStatus {
            online: true,
            on: is_on,
            electricity,
            hardware: Some(MerossHardware {
                device_type: system.all.system.hardware.device_type,
                version: system.all.system.hardware.version,
                chip_type: system.all.system.hardware.chip_type,
                uuid: system.all.system.hardware.uuid,
                mac: system.all.system.hardware.mac_address,
            }),
            firmware: Some(MerossFirmware {
                version: system.all.system.firmware.version,
                compile_time: system.all.system.firmware.compile_time,
                inner_ip: system.all.system.firmware.inner_ip,
            }),
            wifi: MerossWifi { signal },
            last_update: now,
        };

        Ok((MerossDeviceRef::from(&device.config), status))
    }

    pub async fn get_electricity(
        &self,
        device_id: &str,
    ) -> Result<(MerossDeviceRef, MerossElectricityFormatted), AppError> {
        let device = self.get_device(device_id)?;
        let raw = self.get_electricity_raw(device).await?;
        let now = now_millis();
        {
            let mut state = device.state.write().await;
            state.is_online = true;
            state.last_ping = now;
        }

        Ok((
            MerossDeviceRef::from(&device.config),
            MerossElectricityFormatted {
                voltage: format_voltage(raw.voltage),
                current: format_current(raw.current),
                power: format_power(raw.power),
                raw,
            },
        ))
    }

    pub async fn get_consumption(
        &self,
        device_id: &str,
    ) -> Result<(MerossDeviceRef, Vec<MerossConsumptionEntry>, MerossConsumptionSummary), AppError> {
        let device = self.get_device(device_id)?;
        let payload: MerossConsumptionPayload = self
            .send_get(device, "Appliance.Control.ConsumptionX", serde_json::json!({}))
            .await?;
        let consumption = payload.consumptionx.unwrap_or_default();
        let total_wh = consumption.iter().map(|entry| entry.value).sum::<i32>();
        let now = now_millis();
        {
            let mut state = device.state.write().await;
            state.is_online = true;
            state.last_ping = now;
        }

        Ok((
            MerossDeviceRef::from(&device.config),
            consumption.clone(),
            MerossConsumptionSummary {
                days: consumption.len(),
                total_wh,
                total_kwh: ((f64::from(total_wh) / 1000.0) * 100.0).round() / 100.0,
            },
        ))
    }

    pub async fn toggle(&self, device_id: &str, on: bool) -> Result<MerossToggleResponse, AppError> {
        let device = self.get_device(device_id)?;

        let toggle_x_payload = serde_json::json!({
            "togglex": {
                "channel": 0,
                "onoff": if on { 1 } else { 0 },
            }
        });

        let toggle_payload = serde_json::json!({
            "channel": 0,
            "toggle": {
                "onoff": if on { 1 } else { 0 },
            }
        });

        if self
            .send_set_raw(device, "Appliance.Control.ToggleX", toggle_x_payload.clone())
            .await
            .is_err()
        {
            self.send_set_raw(device, "Appliance.Control.Toggle", toggle_payload)
                .await?;
        }

        let now = now_millis();
        {
            let mut state = device.state.write().await;
            state.is_online = true;
            state.is_on = on;
            state.last_ping = now;
        }

        Ok(MerossToggleResponse {
            device: MerossDeviceRef::from(&device.config),
            on,
            message: format!("{} turned {}", device.config.name, if on { "on" } else { "off" }),
        })
    }

    pub async fn set_dnd(&self, device_id: &str, enabled: bool) -> Result<MerossDndResponse, AppError> {
        let device = self.get_device(device_id)?;
        let payload = serde_json::json!({
            "DNDMode": {
                "mode": if enabled { 1 } else { 0 },
            }
        });
        self.send_set_raw(device, "Appliance.System.DNDMode", payload).await?;

        let now = now_millis();
        {
            let mut state = device.state.write().await;
            state.is_online = true;
            state.last_ping = now;
        }

        Ok(MerossDndResponse {
            device: MerossDeviceRef::from(&device.config),
            dnd_mode: enabled,
            message: format!(
                "DND mode {} (LED {})",
                if enabled { "enabled" } else { "disabled" },
                if enabled { "off" } else { "on" }
            ),
        })
    }

    fn get_device(&self, device_id: &str) -> Result<&MerossManagedDevice, AppError> {
        self.devices
            .get(device_id)
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Device not found"))
    }

    async fn get_system_all(&self, device: &MerossManagedDevice) -> Result<MerossSystemAllPayload, AppError> {
        self.send_get(device, "Appliance.System.All", serde_json::json!({})).await
    }

    async fn get_runtime_signal(&self, device: &MerossManagedDevice) -> Result<Option<i32>, AppError> {
        let payload: MerossRuntimePayload = self
            .send_get(device, "Appliance.System.Runtime", serde_json::json!({}))
            .await?;
        Ok(payload.runtime.signal)
    }

    async fn get_electricity_raw(&self, device: &MerossManagedDevice) -> Result<MerossElectricityRaw, AppError> {
        let payload: MerossElectricityPayload = self
            .send_get(
                device,
                "Appliance.Control.Electricity",
                serde_json::json!({ "electricity": { "channel": 0 } }),
            )
            .await?;
        Ok(payload.electricity)
    }

    async fn send_get<T: for<'de> Deserialize<'de>>(
        &self,
        device: &MerossManagedDevice,
        namespace: &'static str,
        payload: serde_json::Value,
    ) -> Result<T, AppError> {
        let packet = build_packet(&device.config, namespace, "GET", payload);
        let response = self
            .client
            .post(format!("http://{}/config", device.config.ip))
            .json(&packet)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let mut state = device.state.write().await;
                state.is_online = false;
                return Err(AppError::Reqwest(error));
            }
        };

        if !response.status().is_success() {
            let mut state = device.state.write().await;
            state.is_online = false;
            return Err(AppError::service_unavailable(format!(
                "Meross device returned {}",
                response.status()
            )));
        }

        let packet = response.json::<MerossPacket>().await?;
        if packet.header.method == "ERROR" {
            let mut state = device.state.write().await;
            state.is_online = false;
            return Err(AppError::service_unavailable("Meross device returned protocol error"));
        }

        serde_json::from_value(packet.payload).map_err(AppError::from)
    }

    async fn send_set_raw(
        &self,
        device: &MerossManagedDevice,
        namespace: &'static str,
        payload: serde_json::Value,
    ) -> Result<(), AppError> {
        let packet = build_packet(&device.config, namespace, "SET", payload);
        let response = self
            .client
            .post(format!("http://{}/config", device.config.ip))
            .json(&packet)
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let mut state = device.state.write().await;
                state.is_online = false;
                return Err(AppError::Reqwest(error));
            }
        };

        if !response.status().is_success() {
            let mut state = device.state.write().await;
            state.is_online = false;
            return Err(AppError::service_unavailable(format!(
                "Meross device returned {}",
                response.status()
            )));
        }

        let packet = response.json::<MerossPacket>().await?;
        if packet.header.method == "ERROR" {
            let mut state = device.state.write().await;
            state.is_online = false;
            return Err(AppError::service_unavailable("Meross device returned protocol error"));
        }

        Ok(())
    }

    async fn snapshot_device(&self, device: &MerossManagedDevice) -> MerossCachedState {
        match self.get_system_all(device).await {
            Ok(system) => {
                let is_on = extract_on_state(&system.all);
                let now = now_millis();
                let mut state = device.state.write().await;
                state.is_online = true;
                state.is_on = is_on;
                state.last_ping = now;
                state.clone()
            }
            Err(_) => {
                let mut state = device.state.write().await;
                state.is_online = false;
                state.clone()
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MerossDeviceRef {
    pub id: String,
    pub name: String,
}

impl From<&MerossDeviceConfig> for MerossDeviceRef {
    fn from(value: &MerossDeviceConfig) -> Self {
        Self {
            id: value.ip.clone(),
            name: value.name.clone(),
        }
    }
}

fn build_packet(
    config: &MerossDeviceConfig,
    namespace: &'static str,
    method: &'static str,
    payload: serde_json::Value,
) -> MerossRequestPacket<'static> {
    let message_id = Uuid::new_v4().simple().to_string();
    let timestamp = chrono::Utc::now().timestamp();
    let timestamp_ms = now_millis() % 1000;
    let sign = compute_sign(&message_id, &config.key, timestamp);

    MerossRequestPacket {
        header: MerossRequestHeader {
            from: format!("http://{}/config", config.ip),
            message_id,
            method,
            namespace,
            payload_version: 1,
            sign,
            timestamp,
            timestamp_ms,
        },
        payload,
    }
}

fn compute_sign(message_id: &str, key: &str, timestamp: i64) -> String {
    let input = format!("{message_id}{key}{timestamp}");
    format!("{:x}", md5::compute(input))
}

fn extract_on_state(system: &MerossSystemAll) -> bool {
    if let Some(togglex) = system
        .control
        .as_ref()
        .and_then(|control| control.togglex.as_ref())
        .and_then(|togglex| togglex.first())
    {
        return togglex.onoff == 1;
    }

    if let Some(toggle) = system.control.as_ref().and_then(|control| control.toggle.as_ref()) {
        return toggle.onoff == 1;
    }

    if let Some(togglex) = system
        .digest
        .as_ref()
        .and_then(|digest| digest.togglex.as_ref())
        .and_then(|togglex| togglex.first())
    {
        return togglex.onoff == 1;
    }

    system
        .digest
        .as_ref()
        .and_then(|digest| digest.toggle.as_ref())
        .is_some_and(|toggle| toggle.onoff == 1)
}

fn format_voltage(raw: i32) -> String {
    trim_float_suffix(f64::from(raw) / 10.0, "V")
}

fn format_current(raw: i32) -> String {
    trim_float_suffix(f64::from(raw) / 1000.0, "A")
}

fn format_power(raw: i32) -> String {
    trim_float_suffix(f64::from(raw) / 1000.0, "W")
}

fn trim_float_suffix(value: f64, suffix: &str) -> String {
    if value.fract() == 0.0 {
        format!("{}{suffix}", value.trunc() as i64)
    } else {
        format!("{}{suffix}", value)
    }
}

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
