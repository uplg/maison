use std::{
    collections::HashMap,
    net::IpAddr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use rust_async_tuyapi::{DpId, Payload, PayloadStruct, mesparse::Message, tuyadevice::TuyaDevice};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::sync::{RwLock, mpsc, oneshot};

use crate::error::AppError;

const STATUS_TIMEOUT_MS: u64 = 12_000;
const MESSAGE_DRAIN_TIMEOUT_MS: u64 = 1_500;
const COMMAND_SETTLE_TIMEOUT_MS: u64 = 400;
const HEARTBEAT_INTERVAL_MS: u64 = 30_000;
const COMMAND_REPLY_TIMEOUT_MS: u64 = 20_000;
const RECONNECT_BASE_DELAY_MS: u64 = 1_000;
const RECONNECT_MAX_DELAY_MS: u64 = 60_000;

#[derive(Debug, Clone, Deserialize)]
pub struct TuyaDeviceConfig {
    pub name: String,
    pub id: String,
    pub key: String,
    pub category: String,
    pub product_name: String,
    pub port: Option<u16>,
    pub model: Option<String>,
    pub ip: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TuyaDeviceListEntry {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: String,
    pub product_name: String,
    pub model: Option<String>,
    pub ip: String,
    pub version: String,
    pub connected: bool,
    pub connecting: bool,
    pub reconnect_attempts: i32,
    pub last_data: TuyaStatusPayload,
    pub parsed_data: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct TuyaStatusPayload {
    pub dps: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TuyaDeviceRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DeviceCache(HashMap<String, HashMap<String, Value>>);

#[derive(Debug, Clone)]
pub struct TuyaManager {
    devices: Arc<HashMap<String, ManagedTuyaDevice>>,
    cache: Arc<RwLock<DeviceCache>>,
    cache_path: Arc<PathBuf>,
    runtime: Arc<RwLock<HashMap<String, RuntimeDeviceState>>>,
}

#[derive(Debug, Clone)]
struct ManagedTuyaDevice {
    config: TuyaDeviceConfig,
    device_type: TuyaDeviceType,
}

#[derive(Debug)]
struct RuntimeDeviceState {
    connected: bool,
    connecting: bool,
    reconnect_attempts: i32,
    last_data: Map<String, Value>,
    parsed_data: Value,
    command_tx: Option<mpsc::Sender<WorkerCommand>>,
}

enum WorkerCommand {
    FetchStatus {
        reply: oneshot::Sender<Result<Map<String, Value>, String>>,
    },
    SetValues {
        updates: Vec<(String, Value)>,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Disconnect {
        reply: oneshot::Sender<()>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuyaDeviceType {
    Feeder,
    LitterBox,
    Fountain,
    Unknown,
}

impl TuyaManager {
    pub fn new(devices_path: &Path, cache_path: &Path) -> Result<Self, AppError> {
        let devices_content = std::fs::read_to_string(devices_path)?;
        let configs = serde_json::from_str::<Vec<TuyaDeviceConfig>>(&devices_content)?;

        let cache = if cache_path.exists() {
            let cache_content = std::fs::read_to_string(cache_path)?;
            DeviceCache(serde_json::from_str(&cache_content)?)
        } else {
            DeviceCache::default()
        };

        let devices = configs
            .into_iter()
            .map(|config| {
                let device_type = determine_device_type(&config);
                (
                    config.id.clone(),
                    ManagedTuyaDevice {
                        config,
                        device_type,
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        let runtime = devices
            .values()
            .map(|device| {
                let cached = cache
                    .0
                    .get(&device.config.id)
                    .map(|values| values.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
                    .unwrap_or_default();
                (
                    device.config.id.clone(),
                    RuntimeDeviceState {
                        connected: false,
                        connecting: false,
                        reconnect_attempts: 0,
                        parsed_data: parse_device_data(device.device_type, &cached),
                        last_data: cached,
                        command_tx: None,
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        Ok(Self {
            devices: Arc::new(devices),
            cache: Arc::new(RwLock::new(cache)),
            cache_path: Arc::new(cache_path.to_path_buf()),
            runtime: Arc::new(RwLock::new(runtime)),
        })
    }

    pub async fn list_devices(&self) -> Vec<TuyaDeviceListEntry> {
        let mut entries = Vec::with_capacity(self.devices.len());

        for device in self.devices.values() {
            let runtime_snapshot = self.runtime_snapshot(&device.config.id).await;
            let (merged, connected, parsed_data, connecting, reconnect_attempts) = if let Some(snapshot) = runtime_snapshot {
                (
                    snapshot.last_data,
                    snapshot.connected,
                    snapshot.parsed_data,
                    snapshot.connecting,
                    snapshot.reconnect_attempts,
                )
            } else {
                let cached = self.cached_dps(&device.config.id).await;
                let parsed = parse_device_data(device.device_type, &cached);
                (cached, false, parsed, false, 0)
            };

            entries.push(TuyaDeviceListEntry {
                id: device.config.id.clone(),
                name: device.config.name.clone(),
                device_type: device.device_type.as_str().to_string(),
                product_name: device.config.product_name.clone(),
                model: device.config.model.clone(),
                ip: device.config.ip.clone(),
                version: device.config.version.clone(),
                connected,
                connecting,
                reconnect_attempts,
                last_data: TuyaStatusPayload { dps: merged.clone() },
                parsed_data,
            });
        }

        entries
    }

    pub async fn get_status(
        &self,
        device_id: &str,
    ) -> Result<(TuyaDeviceRef, Map<String, Value>, Value), AppError> {
        let device = self
            .devices
            .get(device_id)
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Device not found"))?;

        let mut dps = self.fetch_device_dps(&device.config, device.device_type).await?;
        let parsed = match device.device_type {
            TuyaDeviceType::Fountain => Value::Null,
            other => parse_device_data(other, &dps),
        };

        Ok((
            TuyaDeviceRef {
                id: device.config.id.clone(),
                name: device.config.name.clone(),
            },
            std::mem::take(&mut dps),
            parsed,
        ))
    }

    pub async fn get_typed_status(
        &self,
        device_id: &str,
        expected: TuyaDeviceType,
    ) -> Result<(TuyaDeviceRef, Map<String, Value>, Value), AppError> {
        let device = self.validate_typed_device(device_id, expected)?;

        let mut dps = self.fetch_device_dps(&device.config, device.device_type).await?;
        let parsed = parse_device_data(device.device_type, &dps);

        Ok((
            TuyaDeviceRef {
                id: device.config.id.clone(),
                name: device.config.name.clone(),
            },
            std::mem::take(&mut dps),
            parsed,
        ))
    }

    pub async fn send_typed_command(
        &self,
        device_id: &str,
        expected: TuyaDeviceType,
        dps: &str,
        value: Value,
    ) -> Result<TuyaDeviceRef, AppError> {
        self.send_typed_commands(device_id, expected, vec![(dps.to_string(), value)])
            .await
    }

    pub fn get_device_ref(&self, device_id: &str) -> Result<TuyaDeviceRef, AppError> {
        let device = self
            .devices
            .get(device_id)
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Device not found"))?;

        Ok(TuyaDeviceRef {
            id: device.config.id.clone(),
            name: device.config.name.clone(),
        })
    }

    pub async fn send_typed_commands(
        &self,
        device_id: &str,
        expected: TuyaDeviceType,
        updates: Vec<(String, Value)>,
    ) -> Result<TuyaDeviceRef, AppError> {
        let device = self.validate_typed_device(device_id, expected)?;

        self.send_device_commands(&device.config, device.device_type, &updates).await?;
        self.store_command_cache_updates(&device.config.id, device.device_type, &updates)
            .await;

        Ok(TuyaDeviceRef {
            id: device.config.id.clone(),
            name: device.config.name.clone(),
        })
    }

    fn validate_typed_device(
        &self,
        device_id: &str,
        expected: TuyaDeviceType,
    ) -> Result<&ManagedTuyaDevice, AppError> {
        let device = self
            .devices
            .get(device_id)
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Device not found"))?;

        if device.device_type != expected {
            let message = match expected {
                TuyaDeviceType::Feeder => "Device is not a feeder",
                TuyaDeviceType::LitterBox => "Device is not a litter box",
                TuyaDeviceType::Fountain => "Device is not a fountain",
                TuyaDeviceType::Unknown => "Unsupported device type",
            };
            return Err(AppError::http(axum::http::StatusCode::BAD_REQUEST, message));
        }

        Ok(device)
    }

    pub async fn feeder_meal_plan(
        &self,
        device_id: &str,
    ) -> Result<(TuyaDeviceRef, Option<String>), AppError> {
        let device = self.validate_typed_device(device_id, TuyaDeviceType::Feeder)?;
        let dps = self.fetch_device_dps(&device.config, device.device_type).await?;

        Ok((
            TuyaDeviceRef {
                id: device.config.id.clone(),
                name: device.config.name.clone(),
            },
            dps.get("1").and_then(Value::as_str).map(ToOwned::to_owned),
        ))
    }

    pub async fn connect_device(&self, device_id: &str) -> Result<bool, AppError> {
        let device = self
            .devices
            .get(device_id)
            .ok_or_else(|| AppError::http(axum::http::StatusCode::NOT_FOUND, "Device not found"))?;

        if self.runtime_command_sender(&device.config.id).await.is_some() {
            return Ok(true);
        }

        let (tx, rx) = mpsc::channel(16);
        self.note_runtime_connect_attempt(&device.config.id, tx.clone()).await;

        let manager = self.clone();
        let config = device.config.clone();
        let device_type = device.device_type;
        let worker_tx = tx.clone();
        tokio::spawn(async move {
            manager.run_device_worker(config, device_type, worker_tx, rx).await;
        });

        tokio::time::timeout(Duration::from_millis(STATUS_TIMEOUT_MS), async {
            loop {
                if let Some(snapshot) = self.runtime_snapshot(&device.config.id).await {
                    if snapshot.connected {
                        return true;
                    }
                    if !snapshot.connecting && snapshot.reconnect_attempts > 0 {
                        return false;
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .unwrap_or(false)
        .then_some(true)
        .ok_or_else(|| AppError::service_unavailable(format!("Failed to connect to {}", device.config.name)))?;

        Ok(true)
    }

    pub async fn disconnect_device(&self, device_id: &str) -> Result<(), AppError> {
        self.get_device_ref(device_id)?;
        if let Some(tx) = self.runtime_command_sender(device_id).await {
            let (reply_tx, reply_rx) = oneshot::channel();
            let _ = tx.send(WorkerCommand::Disconnect { reply: reply_tx }).await;
            let _ = tokio::time::timeout(Duration::from_millis(STATUS_TIMEOUT_MS), reply_rx).await;
        }
        self.mark_runtime_disconnected(device_id, true).await;
        Ok(())
    }

    pub async fn reconnect_disconnected(&self) {
        let device_ids = {
            let runtime = self.runtime.read().await;
            runtime
                .iter()
                .filter(|(_, state)| !state.connected)
                .map(|(device_id, _)| device_id.clone())
                .collect::<Vec<_>>()
        };

        for device_id in device_ids {
            let _ = self.connect_device(&device_id).await;
        }
    }

    pub async fn disconnect_all_devices(&self) {
        let device_ids = self.devices.keys().cloned().collect::<Vec<_>>();
        for device_id in device_ids {
            self.mark_runtime_disconnected(&device_id, true).await;
        }
    }

    pub async fn connection_stats(&self) -> TuyaConnectionStats {
        let runtime = self.runtime.read().await;
        let devices = self
            .devices
            .values()
            .map(|device| {
                let state = runtime.get(&device.config.id);
                TuyaConnectionStatsEntry {
                    id: device.config.id.clone(),
                    name: device.config.name.clone(),
                    device_type: device.device_type.as_str().to_string(),
                    connected: state.map(|state| state.connected).unwrap_or(false),
                    connecting: state.map(|state| state.connecting).unwrap_or(false),
                    reconnect_attempts: state.map(|state| state.reconnect_attempts).unwrap_or_default(),
                }
            })
            .collect::<Vec<_>>();
        let connected = devices.iter().filter(|device| device.connected).count();

        TuyaConnectionStats {
            total: devices.len(),
            connected,
            disconnected: devices.len().saturating_sub(connected),
            devices,
        }
    }

    async fn fetch_device_dps(
        &self,
        config: &TuyaDeviceConfig,
        device_type: TuyaDeviceType,
    ) -> Result<Map<String, Value>, AppError> {
        let mut merged = self.fetch_device_dps_via_worker(config, device_type).await?;
        self.store_cacheable_dps(&config.id, device_type, &merged).await;
        self.merge_cached_dps(&mut merged, device_type, &config.id).await;
        Ok(merged)
    }

    async fn cached_dps(&self, device_id: &str) -> Map<String, Value> {
        self.cache
            .read()
            .await
            .0
            .get(device_id)
            .map(|values| values.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
            .unwrap_or_default()
    }

    async fn merge_cached_dps(
        &self,
        merged: &mut Map<String, Value>,
        device_type: TuyaDeviceType,
        device_id: &str,
    ) {
        let cache = self.cache.read().await;
        let Some(cached) = cache.0.get(device_id) else {
            return;
        };

        for dps_id in cacheable_dps_keys(device_type) {
            if let Some(value) = cached.get(*dps_id) {
                merged.entry((*dps_id).to_string()).or_insert_with(|| value.clone());
            }
        }
    }

    async fn store_cacheable_dps(
        &self,
        device_id: &str,
        device_type: TuyaDeviceType,
        dps: &Map<String, Value>,
    ) {
        let updates = cacheable_dps_keys(device_type)
            .iter()
            .filter_map(|key| dps.get(*key).cloned().map(|value| ((*key).to_string(), value)))
            .collect::<Vec<_>>();
        self.apply_cache_updates(device_id, updates).await;
    }

    async fn store_command_cache_updates(
        &self,
        device_id: &str,
        device_type: TuyaDeviceType,
        updates: &[(String, Value)],
    ) {
        let cacheable = updates
            .iter()
            .filter(|(dps_id, _)| cacheable_dps_keys(device_type).contains(&dps_id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        self.apply_cache_updates(device_id, cacheable).await;
    }

    async fn apply_cache_updates(&self, device_id: &str, updates: Vec<(String, Value)>) {
        if updates.is_empty() {
            return;
        }

        let snapshot = {
            let mut cache = self.cache.write().await;
            let entry = cache.0.entry(device_id.to_string()).or_default();
            for (key, value) in updates {
                entry.insert(key, value);
            }
            cache.clone()
        };

        let _ = save_device_cache(&self.cache_path, &snapshot);
    }

    async fn send_device_commands(
        &self,
        config: &TuyaDeviceConfig,
        device_type: TuyaDeviceType,
        updates: &[(String, Value)],
    ) -> Result<(), AppError> {
        let tx = self
            .runtime_command_sender(&config.id)
            .await
            .ok_or_else(|| AppError::service_unavailable(format!("{} is not connected", config.name)))?;

        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(WorkerCommand::SetValues {
            updates: updates.to_vec(),
            reply: reply_tx,
        })
        .await
        .map_err(|_| AppError::service_unavailable(format!("{} worker is unavailable", config.name)))?;

        tokio::time::timeout(Duration::from_millis(COMMAND_REPLY_TIMEOUT_MS), reply_rx)
            .await
            .map_err(|_| AppError::service_unavailable(format!("Command timeout for {}", config.name)))?
            .map_err(|_| AppError::service_unavailable(format!("{} worker dropped command reply", config.name)))?
            .map_err(AppError::service_unavailable)?;

        let mut merged = self.cached_dps(&config.id).await;
        for (key, value) in updates {
            merged.insert(key.clone(), value.clone());
        }
        self.update_runtime_snapshot(&config.id, device_type, true, merged).await;
        Ok(())
    }

    async fn runtime_command_sender(&self, device_id: &str) -> Option<mpsc::Sender<WorkerCommand>> {
        self.runtime
            .read()
            .await
            .get(device_id)
            .and_then(|state| state.command_tx.clone())
    }

    async fn runtime_snapshot(&self, device_id: &str) -> Option<RuntimeSnapshot> {
        self.runtime.read().await.get(device_id).map(|state| RuntimeSnapshot {
            connected: state.connected,
            connecting: state.connecting,
            reconnect_attempts: state.reconnect_attempts,
            last_data: state.last_data.clone(),
            parsed_data: state.parsed_data.clone(),
        })
    }

    async fn note_runtime_connect_attempt(&self, device_id: &str, command_tx: mpsc::Sender<WorkerCommand>) {
        let mut runtime = self.runtime.write().await;
        if let Some(state) = runtime.get_mut(device_id) {
            state.connecting = true;
            state.command_tx = Some(command_tx);
            if !state.connected {
                state.reconnect_attempts += 1;
            }
        }
    }

    async fn note_runtime_retry_attempt(&self, device_id: &str, command_tx: mpsc::Sender<WorkerCommand>) {
        let mut runtime = self.runtime.write().await;
        if let Some(state) = runtime.get_mut(device_id) {
            state.connecting = true;
            state.command_tx = Some(command_tx);
            state.reconnect_attempts += 1;
        }
    }

    async fn mark_runtime_connected(&self, device_id: &str) {
        let mut runtime = self.runtime.write().await;
        if let Some(state) = runtime.get_mut(device_id) {
            state.connected = true;
            state.connecting = false;
            state.reconnect_attempts = 0;
        }
    }

    async fn mark_runtime_failed(&self, device_id: &str, clear_sender: bool) {
        let mut runtime = self.runtime.write().await;
        if let Some(state) = runtime.get_mut(device_id) {
            state.connected = false;
            state.connecting = false;
            if clear_sender {
                state.command_tx = None;
            }
        }
    }

    async fn mark_runtime_disconnected(&self, device_id: &str, clear_sender: bool) {
        let mut runtime = self.runtime.write().await;
        if let Some(state) = runtime.get_mut(device_id) {
            state.connected = false;
            state.connecting = false;
            if clear_sender {
                state.command_tx = None;
            }
        }
    }

    async fn update_runtime_snapshot(
        &self,
        device_id: &str,
        device_type: TuyaDeviceType,
        connected: bool,
        dps: Map<String, Value>,
    ) {
        let mut runtime = self.runtime.write().await;
        if let Some(state) = runtime.get_mut(device_id) {
            state.connected = connected;
            state.connecting = false;
            state.last_data = dps.clone();
            state.parsed_data = parse_device_data(device_type, &dps);
        }
    }

    async fn fetch_device_dps_via_worker(
        &self,
        config: &TuyaDeviceConfig,
        device_type: TuyaDeviceType,
    ) -> Result<Map<String, Value>, AppError> {
        if self.runtime_command_sender(&config.id).await.is_none() {
            self.connect_device(&config.id).await?;
        }

        let tx = self
            .runtime_command_sender(&config.id)
            .await
            .ok_or_else(|| AppError::service_unavailable(format!("{} is not connected", config.name)))?;

        let (reply_tx, reply_rx) = oneshot::channel();
        tx.send(WorkerCommand::FetchStatus { reply: reply_tx })
            .await
            .map_err(|_| AppError::service_unavailable(format!("{} worker is unavailable", config.name)))?;

        let dps = tokio::time::timeout(Duration::from_millis(COMMAND_REPLY_TIMEOUT_MS), reply_rx)
            .await
            .map_err(|_| AppError::service_unavailable(format!("Status request timeout for {}", config.name)))?
            .map_err(|_| AppError::service_unavailable(format!("{} worker dropped status reply", config.name)))?
            .map_err(AppError::service_unavailable)?;

        self.update_runtime_snapshot(&config.id, device_type, true, dps.clone()).await;
        Ok(dps)
    }

    async fn run_device_worker(
        self,
        config: TuyaDeviceConfig,
        device_type: TuyaDeviceType,
        command_tx: mpsc::Sender<WorkerCommand>,
        mut command_rx: mpsc::Receiver<WorkerCommand>,
    ) {
        loop {
            let run_result = self
                .run_device_worker_session(config.clone(), device_type, &mut command_rx)
                .await;

            match run_result {
                WorkerLoopResult::Shutdown => {
                    self.mark_runtime_disconnected(&config.id, true).await;
                    break;
                }
                WorkerLoopResult::Retry => {
                    let attempts = self.runtime_snapshot(&config.id).await.map(|s| s.reconnect_attempts).unwrap_or(1);
                    let delay = reconnect_delay(attempts);
                    tokio::time::sleep(delay).await;
                    self.note_runtime_retry_attempt(&config.id, command_tx.clone()).await;
                }
            }
        }
    }

    async fn run_device_worker_session(
        &self,
        config: TuyaDeviceConfig,
        device_type: TuyaDeviceType,
        command_rx: &mut mpsc::Receiver<WorkerCommand>,
    ) -> WorkerLoopResult {
        let ip = match IpAddr::from_str(&config.ip) {
            Ok(ip) => ip,
            Err(_) => {
                self.mark_runtime_failed(&config.id, false).await;
                return WorkerLoopResult::Retry;
            }
        };

        let mut device = match TuyaDevice::new(&config.version, &config.id, Some(&config.key), ip) {
            Ok(device) => device,
            Err(_) => {
                self.mark_runtime_failed(&config.id, false).await;
                return WorkerLoopResult::Retry;
            }
        };

        let mut rx = match tokio::time::timeout(Duration::from_millis(STATUS_TIMEOUT_MS), device.connect()).await {
            Ok(Ok(rx)) => rx,
            _ => {
                self.mark_runtime_failed(&config.id, false).await;
                return WorkerLoopResult::Retry;
            }
        };

        self.mark_runtime_connected(&config.id).await;
        if let Ok(dps) = worker_fetch_status(&mut device, &mut rx, &config).await {
            self.store_cacheable_dps(&config.id, device_type, &dps).await;
            self.update_runtime_snapshot(&config.id, device_type, true, dps).await;
        }

        let mut heartbeat = tokio::time::interval(Duration::from_millis(HEARTBEAT_INTERVAL_MS));

        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    if device.heartbeat().await.is_err() {
                        self.mark_runtime_failed(&config.id, false).await;
                        let _ = device.disconnect().await;
                        return WorkerLoopResult::Retry;
                    }
                }
                maybe_messages = rx.recv() => {
                    match maybe_messages {
                        Some(Ok(messages)) => {
                            let current = self.runtime_snapshot(&config.id).await.map(|snapshot| snapshot.last_data).unwrap_or_default();
                            let merged = merge_messages_into_dps(current, messages);
                            self.store_cacheable_dps(&config.id, device_type, &merged).await;
                            self.update_runtime_snapshot(&config.id, device_type, true, merged).await;
                        }
                        Some(Err(_)) | None => {
                            self.mark_runtime_failed(&config.id, false).await;
                            let _ = device.disconnect().await;
                            return WorkerLoopResult::Retry;
                        }
                    }
                }
                Some(command) = command_rx.recv() => {
                    match command {
                        WorkerCommand::FetchStatus { reply } => {
                            let _ = reply.send(worker_fetch_status(&mut device, &mut rx, &config).await.map_err(|err| err.to_string()));
                        }
                        WorkerCommand::SetValues { updates, reply } => {
                            let result = worker_send_commands(&mut device, &mut rx, &config, &updates).await;
                            let _ = reply.send(result.map_err(|err| err.to_string()));
                        }
                        WorkerCommand::Disconnect { reply } => {
                            let _ = device.disconnect().await;
                            let _ = reply.send(());
                            return WorkerLoopResult::Shutdown;
                        }
                    }
                }
                else => {
                    let _ = device.disconnect().await;
                    return WorkerLoopResult::Shutdown;
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct RuntimeSnapshot {
    connected: bool,
    connecting: bool,
    reconnect_attempts: i32,
    last_data: Map<String, Value>,
    parsed_data: Value,
}

enum WorkerLoopResult {
    Retry,
    Shutdown,
}

fn reconnect_delay(attempts: i32) -> Duration {
    let exponent = attempts.max(1).saturating_sub(1) as u32;
    let delay_ms = RECONNECT_BASE_DELAY_MS
        .saturating_mul(2_u64.saturating_pow(exponent))
        .min(RECONNECT_MAX_DELAY_MS);
    Duration::from_millis(delay_ms)
}

async fn worker_fetch_status(
    device: &mut TuyaDevice,
    rx: &mut tokio::sync::mpsc::Receiver<rust_async_tuyapi::Result<Vec<Message>>>,
    config: &TuyaDeviceConfig,
) -> Result<Map<String, Value>, AppError> {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| AppError::service_unavailable(error.to_string()))?
        .as_secs()
        .to_string();

    let payload = Payload::Struct(PayloadStruct {
        gw_id: Some(config.id.clone()),
        dev_id: config.id.clone(),
        uid: Some(config.id.clone()),
        t: Some(current_time.clone()),
        dp_id: None,
        dps: Some(json!({})),
    });

    tokio::time::timeout(Duration::from_millis(STATUS_TIMEOUT_MS), device.get(payload))
        .await
        .map_err(|_| AppError::service_unavailable(format!("Status request timeout for {}", config.name)))?
        .map_err(|error| AppError::service_unavailable(error.to_string()))?;

    let received = tokio::time::timeout(Duration::from_millis(STATUS_TIMEOUT_MS), rx.recv())
        .await
        .map_err(|_| AppError::service_unavailable(format!("Status request timeout for {}", config.name)))?
        .ok_or_else(|| AppError::service_unavailable(format!("No response from {}", config.name)))?
        .map_err(|error| AppError::service_unavailable(error.to_string()))?;

    let refresh_payload = Payload::new(
        config.id.clone(),
        Some(config.id.clone()),
        Some(config.id.clone()),
        Some(current_time.parse::<u32>().map_err(|error| AppError::service_unavailable(error.to_string()))?),
        Some(DpId::Higher),
        None,
    );
    let _ = tokio::time::timeout(Duration::from_millis(STATUS_TIMEOUT_MS), device.refresh(refresh_payload)).await;
    let maybe_refresh = tokio::time::timeout(Duration::from_millis(MESSAGE_DRAIN_TIMEOUT_MS), rx.recv())
        .await
        .ok();

    let mut merged = Map::new();
    for message in received {
        merge_payload_dps(&mut merged, message.payload);
    }
    if let Some(Some(Ok(messages))) = maybe_refresh {
        for message in messages {
            merge_payload_dps(&mut merged, message.payload);
        }
    }

    while let Ok(Some(Ok(messages))) = tokio::time::timeout(Duration::from_millis(400), rx.recv()).await {
        for message in messages {
            merge_payload_dps(&mut merged, message.payload);
        }
    }

    Ok(merged)
}

async fn worker_send_commands(
    device: &mut TuyaDevice,
    rx: &mut tokio::sync::mpsc::Receiver<rust_async_tuyapi::Result<Vec<Message>>>,
    config: &TuyaDeviceConfig,
    updates: &[(String, Value)],
) -> Result<(), AppError> {
    for (dps, value) in updates {
        let mut payload = Map::new();
        payload.insert(dps.clone(), value.clone());

        tokio::time::timeout(
            Duration::from_millis(STATUS_TIMEOUT_MS),
            device.set_values(Value::Object(payload)),
        )
        .await
        .map_err(|_| AppError::service_unavailable(format!("Command timeout for {}", config.name)))?
        .map_err(|error| AppError::service_unavailable(error.to_string()))?;

        let _ = tokio::time::timeout(Duration::from_millis(COMMAND_SETTLE_TIMEOUT_MS), rx.recv()).await;
    }

    Ok(())
}

fn merge_messages_into_dps(
    mut current: Map<String, Value>,
    messages: Vec<Message>,
) -> Map<String, Value> {
    for message in messages {
        merge_payload_dps(&mut current, message.payload);
    }
    current
}

#[derive(Debug, Clone)]
pub struct TuyaConnectionStats {
    pub total: usize,
    pub connected: usize,
    pub disconnected: usize,
    pub devices: Vec<TuyaConnectionStatsEntry>,
}

#[derive(Debug, Clone)]
pub struct TuyaConnectionStatsEntry {
    pub id: String,
    pub name: String,
    pub device_type: String,
    pub connected: bool,
    pub connecting: bool,
    pub reconnect_attempts: i32,
}

fn cacheable_dps_keys(device_type: TuyaDeviceType) -> &'static [&'static str] {
    match device_type {
        TuyaDeviceType::Feeder => &["1"],
        TuyaDeviceType::LitterBox => &["112"],
        TuyaDeviceType::Fountain | TuyaDeviceType::Unknown => &[],
    }
}

fn save_device_cache(path: &Path, cache: &DeviceCache) -> Result<(), AppError> {
    let content = serde_json::to_vec_pretty(cache)?;
    std::fs::write(path, content)?;
    Ok(())
}

fn merge_payload_dps(merged: &mut Map<String, Value>, payload: Payload) {
    match payload {
        Payload::Struct(payload) => {
            if let Some(dps) = payload.dps {
                if let Some(object) = dps.as_object() {
                    for (key, value) in object {
                        merged.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        Payload::String(raw) => {
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                if let Some(object) = value.get("dps").and_then(Value::as_object) {
                    for (key, value) in object {
                        merged.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        Payload::ControlNewStruct(payload) => {
            if let Ok(value) = serde_json::to_value(payload) {
                if let Some(object) = value
                    .get("data")
                    .and_then(|data| data.get("dps"))
                    .and_then(Value::as_object)
                {
                    for (key, value) in object {
                        merged.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        Payload::Raw(_) => {}
    }
}

fn determine_device_type(config: &TuyaDeviceConfig) -> TuyaDeviceType {
    let product_name = config.product_name.to_lowercase();
    let category = config.category.to_lowercase();
    if product_name.contains("feeder") || category == "cwwsq" {
        TuyaDeviceType::Feeder
    } else if product_name.contains("litter") || category == "msp" {
        TuyaDeviceType::LitterBox
    } else if product_name.contains("fountain") || category == "cwysj" {
        TuyaDeviceType::Fountain
    } else {
        TuyaDeviceType::Unknown
    }
}

impl TuyaDeviceType {
    fn as_str(self) -> &'static str {
        match self {
            TuyaDeviceType::Feeder => "feeder",
            TuyaDeviceType::LitterBox => "litter-box",
            TuyaDeviceType::Fountain => "fountain",
            TuyaDeviceType::Unknown => "unknown",
        }
    }
}

fn parse_device_data(device_type: TuyaDeviceType, dps: &Map<String, Value>) -> Value {
    match device_type {
        TuyaDeviceType::Feeder => parse_feeder_status(dps),
        TuyaDeviceType::LitterBox => parse_litter_status(dps),
        TuyaDeviceType::Fountain => parse_fountain_status(dps),
        TuyaDeviceType::Unknown => Value::Object(Map::new()),
    }
}

fn parse_feeder_status(dps: &Map<String, Value>) -> Value {
    let history = dps.get("104").and_then(Value::as_str).map(|history| {
        let parts = history.split("  ").collect::<Vec<_>>();
        json!({
            "raw": history,
            "parsed": {
                "remaining": parts.first().map(|part| part.replace("R:", "")).unwrap_or_default(),
                "count": parts.get(1).map(|part| part.replace("C:", "")),
                "timestamp": parts.get(2).map(|part| part.replace("T:", "")),
                "timestamp_readable": "",
            }
        })
    });

    let feed_size = dps
        .get("101")
        .and_then(Value::as_i64)
        .map(|size| format!("{size} portion{}", if size > 1 { "s" } else { "" }))
        .unwrap_or_else(|| "Unknown".to_string());

    let powered_by = match dps.get("105").and_then(Value::as_i64) {
        Some(0) => "AC Power".to_string(),
        Some(1) => "Battery".to_string(),
        Some(mode) => format!("Mode {mode}"),
        None => "Unknown".to_string(),
    };

    json!({
        "feeding": {
            "manual_feed_enabled": dps.get("102").cloned().unwrap_or(Value::Bool(true)),
            "last_feed_size": feed_size,
            "last_feed_report": dps.get("15").cloned().unwrap_or(json!(0)),
            "quick_feed_available": dps.get("2").cloned().unwrap_or(Value::Bool(false)),
        },
        "settings": {
            "sound_enabled": dps.get("103").cloned().unwrap_or(Value::Bool(true)),
            "alexa_feed_enabled": dps.get("106").cloned().unwrap_or(Value::Bool(false)),
        },
        "system": {
            "fault_status": json!(dps.get("14").and_then(Value::as_i64).unwrap_or_default() != 0),
            "powered_by": powered_by,
            "ip_address": dps.get("107").cloned().unwrap_or(json!("Unknown")),
        },
        "history": history,
    })
}

fn parse_litter_status(dps: &Map<String, Value>) -> Value {
    let clean_delay = dps.get("101").and_then(Value::as_i64).unwrap_or_default();
    let start_minutes = dps.get("103").and_then(Value::as_i64).unwrap_or_default();
    let end_minutes = dps.get("104").and_then(Value::as_i64).unwrap_or_default();

    json!({
        "clean_delay": {
            "seconds": clean_delay,
            "formatted": format_seconds(clean_delay),
        },
        "sleep_mode": {
            "enabled": dps.get("102").cloned().unwrap_or(Value::Bool(false)),
            "start_time_minutes": start_minutes,
            "start_time_formatted": format_minutes(start_minutes),
            "end_time_minutes": end_minutes,
            "end_time_formatted": format_minutes(end_minutes),
        },
        "sensors": {
            "defecation_duration": dps.get("106").cloned().unwrap_or(json!(0)),
            "defecation_frequency": dps.get("105").cloned().unwrap_or(json!(0)),
            "fault_alarm": dps.get("114").cloned().unwrap_or(json!(0)),
            "litter_level": dps.get("112").cloned().unwrap_or(json!("unknown")),
        },
        "system": {
            "state": dps.get("109").cloned().unwrap_or(json!("unknown")),
            "cleaning_in_progress": dps.get("107").cloned().unwrap_or(Value::Bool(false)),
            "maintenance_required": dps.get("108").cloned().unwrap_or(Value::Bool(false)),
        },
        "settings": {
            "lighting": dps.get("116").cloned().unwrap_or(Value::Bool(false)),
            "child_lock": dps.get("110").cloned().unwrap_or(Value::Bool(false)),
            "prompt_sound": dps.get("117").cloned().unwrap_or(Value::Bool(false)),
            "kitten_mode": dps.get("111").cloned().unwrap_or(Value::Bool(false)),
            "automatic_homing": dps.get("119").cloned().unwrap_or(Value::Bool(false)),
        },
    })
}

fn parse_fountain_status(dps: &Map<String, Value>) -> Value {
    let mut parsed = Map::new();
    for (dps_id, field) in [
        ("1", "power"),
        ("3", "water_time"),
        ("4", "filter_life"),
        ("5", "pump_time"),
        ("6", "water_reset"),
        ("7", "filter_reset"),
        ("8", "pump_reset"),
        ("10", "uv"),
        ("11", "uv_runtime"),
        ("12", "water_level"),
        ("101", "low_water"),
        ("102", "eco_mode"),
        ("103", "eco_watering_status"),
        ("104", "no_water"),
        ("110", "associated_camera"),
        ("130", "mac_address"),
    ] {
        if let Some(value) = dps.get(dps_id) {
            parsed.insert(field.to_string(), value.clone());
        }
    }
    Value::Object(parsed)
}

fn format_seconds(seconds: i64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn format_minutes(minutes: i64) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}
