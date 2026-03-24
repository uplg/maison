use std::{collections::HashMap, sync::Arc, time::Duration as StdDuration};

use ashv2::{Actor as AshActor, BaudRate, FlowControl, NativeSerialPort, Payload, Tasks as AshTasks, open as open_ash_serial};
use chrono::Utc;
use ezsp::{
    Callback, Configuration, Ezsp, Messaging, Networking, Security, Utilities,
    ember::{
        Eui64, NodeId,
        aps::{Frame as EzspApsFrame, Options as EzspApsOptions},
        device::Update as EmberDeviceUpdate,
        join::Method as EmberJoinMethod,
        key::Data as EmberKeyData,
        message::Destination,
        network::{Duration as NetworkDuration, Parameters as EmberNetworkParameters, Status as EmberNetworkStatus},
        security::initial,
    },
    ezsp::{config, decision, network::InitBitmask as NetworkInitBitmask, policy},
    parameters,
    uart::Uart as EzspUart,
};
use tokio::{
    sync::{Mutex, RwLock, mpsc, oneshot},
    task::JoinHandle,
    time::{Instant, MissedTickBehavior, interval, timeout},
};
use tracing::{debug, error, info, warn};

use crate::error::AppError;

const DEFAULT_EZSP_PROTOCOL_VERSION: u8 = 13;
const EZSP_CHANNEL_SIZE: usize = 64;
const EZSP_INIT_TIMEOUT: StdDuration = StdDuration::from_secs(5);
const POLL_INTERVAL: StdDuration = StdDuration::from_millis(200);
const DISCOVERY_RETRY_INTERVAL_TICKS: u32 = 10;
const ZDO_PROFILE_ID: u16 = 0x0000;
const ZCL_GLOBAL_FRAME_CONTROL: u8 = 0x00;
const ZCL_CLUSTER_COMMAND_FRAME_CONTROL: u8 = 0x11;
const ZCL_READ_ATTRIBUTES_COMMAND_ID: u8 = 0x00;
const ZCL_READ_ATTRIBUTES_RESPONSE_COMMAND_ID: u8 = 0x01;
const ZCL_ON_OFF_COMMAND_OFF: u8 = 0x00;
const ZCL_ON_OFF_COMMAND_ON: u8 = 0x01;
const ZCL_LEVEL_CONTROL_COMMAND_MOVE_TO_LEVEL: u8 = 0x04;
const ZCL_COLOR_CONTROL_COMMAND_MOVE_TO_COLOR_TEMPERATURE: u8 = 0x0a;
const BASIC_CLUSTER_ID: u16 = 0x0000;
const HOME_AUTOMATION_PROFILE_ID: u16 = 0x0104;
const SIMPLE_DESC_REQ_CLUSTER_ID: u16 = 0x0004;
const ACTIVE_EP_REQ_CLUSTER_ID: u16 = 0x0005;
const DEVICE_ANNCE_CLUSTER_ID: u16 = 0x0013;
const SIMPLE_DESC_RSP_CLUSTER_ID: u16 = 0x8004;
const ACTIVE_EP_RSP_CLUSTER_ID: u16 = 0x8005;
const ON_OFF_CLUSTER_ID: u16 = 0x0006;
const LEVEL_CONTROL_CLUSTER_ID: u16 = 0x0008;
const COLOR_CONTROL_CLUSTER_ID: u16 = 0x0300;
const DEFAULT_SOURCE_ENDPOINT: u8 = 1;
const DEFAULT_HOME_GATEWAY_DEVICE_ID: u16 = 0x0050;
const DEFAULT_STACK_PROFILE: u16 = 2;
const DEFAULT_SECURITY_LEVEL: u16 = 5;
const DEFAULT_NETWORK_CHANNEL: u8 = 11;
const DEFAULT_NETWORK_TX_POWER: u8 = 8;
const DEFAULT_LOCAL_INPUT_CLUSTERS: &[u16] = &[0x0000, 0x0006, 0x0008, 0x0300, 0x0403, 0x0201];
const DEFAULT_LOCAL_OUTPUT_CLUSTERS: &[u16] = &[0x0000, 0x0006, 0x0008, 0x0300, 0x0403];
const ZIGBEE_ALLIANCE09_LINK_KEY: EmberKeyData = *b"ZigBeeAlliance09";

/// Interval between liveness probes in tick counts (150 ticks * 200ms = 30 seconds).
const LIVENESS_PROBE_INTERVAL_TICKS: u32 = 150;
/// Duration after which a device with no response is considered unreachable.
const LIVENESS_TIMEOUT: StdDuration = StdDuration::from_secs(90);

/// Duration after which the EZSP pipeline is considered stuck if no EZSP activity
/// (successful command or callback) has been observed.  Triggers a full reconnect.
const WATCHDOG_TIMEOUT: StdDuration = StdDuration::from_secs(180);

/// Per-EZSP-command timeout.  If a single `send_unicast` / `communicate` call takes
/// longer than this, we assume the pipeline is dead and trigger a full reconnect.
/// This is safe ONLY because we tear down the entire pipeline afterward — we never
/// reuse a desynchronised EZSP channel.
const EZSP_COMMAND_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// Delay before attempting to reconnect after a pipeline failure.
const RECONNECT_DELAY: StdDuration = StdDuration::from_secs(2);

/// Maximum number of consecutive reconnect attempts before giving up.
const MAX_RECONNECT_ATTEMPTS: u32 = 10;

#[derive(Debug, Clone)]
pub enum NativeZigbeeCommand {
    PermitJoin { seconds: u16 },
    DiscoverDevices,
    GetLampState { lamp_id: String },
    SetPower { lamp_id: String, enabled: bool },
    SetBrightness { lamp_id: String, brightness: u8 },
    SetTemperature { lamp_id: String, temperature: u8 },
}

#[derive(Debug, Clone)]
pub enum NativeZigbeeEvent {
    TransportReady,
    NetworkState { status: String },
    DeviceJoined { node_id: u16, eui64: String },
    DeviceAnnounced { node_id: u16, eui64: String },
    IncomingMessage { node_id: u16, cluster_id: u16, payload: Vec<u8> },
}

#[derive(Debug, Clone)]
pub struct NativeDiscoveredDevice {
    pub id: String,
    pub node_id: u16,
    pub eui64: String,
    pub endpoint: Option<u8>,
    pub input_clusters: Vec<u16>,
    pub output_clusters: Vec<u16>,
    pub supports_brightness: bool,
    pub supports_temperature: bool,
    pub connected: bool,
    pub reachable: bool,
    pub is_on: bool,
    pub brightness: u8,
    pub temperature: Option<u8>,
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NativeKnownDevice {
    pub node_id: u16,
    pub eui64: String,
    pub endpoint: Option<u8>,
    pub input_clusters: Vec<u16>,
    pub output_clusters: Vec<u16>,
    pub model: Option<String>,
    pub manufacturer: Option<String>,
    pub supports_brightness: bool,
    pub supports_temperature: bool,
}

#[derive(Debug, Default)]
pub(crate) struct NativeZigbeeStatus {
    connected: bool,
    message: Option<String>,
    last_error: Option<String>,
    devices: Vec<NativeDiscoveredDevice>,
}

struct DriverRequest {
    command: NativeZigbeeCommand,
    reply_tx: oneshot::Sender<Result<(), AppError>>,
}

pub(crate) enum DriverLifecycle {
    Starting,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriverNetworkState {
    Unknown,
    NoNetwork,
    Joined,
}

#[derive(Clone)]
pub struct NativeZigbeeRuntime {
    status: Arc<RwLock<NativeZigbeeStatus>>,
    command_tx: mpsc::Sender<DriverRequest>,
    command_rx: Arc<Mutex<Option<mpsc::Receiver<DriverRequest>>>>,
    task: Arc<std::sync::Mutex<Option<JoinHandle<()>>>>,
    init_once: Arc<Mutex<bool>>,
    lifecycle: Arc<RwLock<DriverLifecycle>>,
    network_state: Arc<RwLock<DriverNetworkState>>,
    adapter: Arc<String>,
    serial_port: Arc<Option<String>>,
    known_devices: Arc<Vec<NativeKnownDevice>>,
}

impl NativeZigbeeRuntime {
    pub fn spawn(adapter: String, serial_port: Option<String>, known_devices: Vec<NativeKnownDevice>) -> Self {
        let adapter_label = adapter.clone();
        let serial_port_label = serial_port.clone();
        let status = Arc::new(RwLock::new(NativeZigbeeStatus {
            connected: false,
            message: Some(match &serial_port {
                Some(port) => format!("Native Zigbee adapter queued for initialization on {port} ({adapter})"),
                None => format!("Native Zigbee adapter {adapter} selected; set ZIGBEE_SERIAL_PORT to enable it"),
            }),
            last_error: None,
            devices: Vec::new(),
        }));

        let (command_tx, command_rx) = mpsc::channel(32);

        Self {
            status,
            command_tx,
            command_rx: Arc::new(Mutex::new(Some(command_rx))),
            task: Arc::new(std::sync::Mutex::new(None)),
            init_once: Arc::new(Mutex::new(false)),
            lifecycle: Arc::new(RwLock::new(DriverLifecycle::Starting)),
            network_state: Arc::new(RwLock::new(DriverNetworkState::Unknown)),
            adapter: Arc::new(adapter_label),
            serial_port: Arc::new(serial_port_label),
            known_devices: Arc::new(known_devices),
        }
    }

    pub async fn is_connected(&self) -> bool {
        self.status.read().await.connected
    }

    pub async fn message(&self) -> Option<String> {
        let status = self.status.read().await;
        status
            .message
            .clone()
            .or_else(|| status.last_error.clone())
    }

    pub async fn snapshot_devices(&self) -> Vec<NativeDiscoveredDevice> {
        self.status.read().await.devices.clone()
    }

    #[cfg(test)]
    pub(crate) async fn test_seed_devices(&self, devices: Vec<NativeDiscoveredDevice>) {
        let mut status = self.status.write().await;
        status.connected = true;
        status.devices = devices;
        status.message = None;
        status.last_error = None;
    }

    #[cfg(test)]
    pub(crate) async fn test_set_lifecycle(&self, lifecycle: DriverLifecycle) {
        *self.lifecycle.write().await = lifecycle;
    }

    #[cfg(test)]
    pub(crate) async fn test_set_network_state(&self, state: &str) {
        *self.network_state.write().await = match state {
            "joined" => DriverNetworkState::Joined,
            "no-network" => DriverNetworkState::NoNetwork,
            _ => DriverNetworkState::Unknown,
        };
    }

    pub async fn ensure_initialized(&self) {
        self.start_task_if_needed().await;

        let mut guard = self.init_once.lock().await;
        if *guard {
            return;
        }

        if let Err(error) = wait_for_driver_ready(&self.lifecycle).await {
            warn!(adapter = %self.adapter, serial_port = ?self.serial_port, error = %error, "native zigbee driver did not become ready");
            let mut status = self.status.write().await;
            status.connected = false;
            status.last_error = Some(error.to_string());
            status.message = Some("Native Zigbee initialization timed out".to_string());
            return;
        }

        if let Err(error) = wait_for_joined_network(&self.network_state).await {
            warn!(adapter = %self.adapter, serial_port = ?self.serial_port, error = %error, "native zigbee network did not become joined before discovery");
            let mut status = self.status.write().await;
            status.connected = false;
            status.last_error = Some(error.to_string());
            status.message = Some(format!("Native Zigbee network not joined: {error}"));
            return;
        }

        if let Err(error) = self.send(NativeZigbeeCommand::DiscoverDevices).await {
            warn!(adapter = %self.adapter, serial_port = ?self.serial_port, error = %error, "native zigbee lazy initialization failed");
            let mut status = self.status.write().await;
            status.connected = false;
            status.last_error = Some(error.to_string());
            status.message = Some("Native Zigbee lazy initialization failed".to_string());
            return;
        }

        *guard = true;
    }

    pub async fn send(&self, command: NativeZigbeeCommand) -> Result<(), AppError> {
        self.start_task_if_needed().await;

        match &*self.lifecycle.read().await {
            DriverLifecycle::Starting => {
                return Err(AppError::service_unavailable(
                    "Native Zigbee adapter is still initializing",
                ));
            }
            DriverLifecycle::Failed(message) => {
                return Err(AppError::service_unavailable(message.clone()));
            }
            DriverLifecycle::Ready => {}
        }

        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(DriverRequest { command, reply_tx })
            .await
            .map_err(|_| AppError::service_unavailable("Native Zigbee driver task is not running"))?;

        reply_rx
            .await
            .map_err(|_| AppError::service_unavailable("Native Zigbee driver dropped the command response"))?
    }

    async fn start_task_if_needed(&self) {
        if self.task.lock().expect("native zigbee task mutex").is_some() {
            return;
        }

        let mut command_rx_guard = self.command_rx.lock().await;
        let Some(command_rx) = command_rx_guard.take() else {
            return;
        };

        let adapter = (*self.adapter).clone();
        let serial_port = (*self.serial_port).clone();
        let known_devices = (*self.known_devices).clone();
        let task_status = Arc::clone(&self.status);
        let lifecycle = Arc::clone(&self.lifecycle);
        let network_state = Arc::clone(&self.network_state);

        let task = tokio::spawn(async move {
            run_native_driver(adapter, serial_port, known_devices, task_status, lifecycle, network_state, command_rx).await;
        });

        *self.task.lock().expect("native zigbee task mutex") = Some(task);
    }

    pub async fn shutdown(&self) {
        if let Some(handle) = self.task.lock().expect("native zigbee task mutex").take() {
            handle.abort();
        }
    }
}

struct EzspContext {
    uart: EzspUart,
    ash_tasks: AshTasks<NativeSerialPort>,
    callbacks_rx: mpsc::UnboundedReceiver<Callback>,
    joined_devices: Vec<DiscoveredDevice>,
    next_global_sequence: u8,
    next_device_sequence: HashMap<u16, u8>,
    /// Updated on every successful EZSP command or callback.  Used by the watchdog
    /// to detect a silently-dead pipeline.
    last_activity: Instant,
}

#[derive(Debug, Clone)]
struct DiscoveredDevice {
    node_id: u16,
    eui64: String,
    endpoint: Option<u8>,
    input_clusters: Vec<u16>,
    output_clusters: Vec<u16>,
    supports_brightness: bool,
    supports_temperature: bool,
    has_color_control_cluster: bool,
    is_on: bool,
    brightness: u8,
    temperature: Option<u8>,
    interview_completed: bool,
    model: Option<String>,
    manufacturer: Option<String>,
    connected: bool,
    reachable: bool,
    interview_attempts: u32,
    last_seen: Option<Instant>,
    /// Last user-requested brightness (0–100).  Re-applied when the device becomes reachable again.
    desired_brightness: Option<u8>,
    /// Last user-requested colour temperature (0–100).  Re-applied on reconnect.
    desired_temperature: Option<u8>,
    /// Set to `false` when the device becomes unreachable; set back to `true` after
    /// `restore_desired_state` successfully re-sends the desired brightness/temperature.
    /// This prevents re-sending every 30 s — we only restore on the unreachable→reachable transition.
    desired_state_applied: bool,
}

async fn run_native_driver(
    adapter: String,
    serial_port: Option<String>,
    known_devices: Vec<NativeKnownDevice>,
    status: Arc<RwLock<NativeZigbeeStatus>>,
    lifecycle: Arc<RwLock<DriverLifecycle>>,
    driver_network_state: Arc<RwLock<DriverNetworkState>>,
    mut command_rx: mpsc::Receiver<DriverRequest>,
) {
    let Some(serial_port) = serial_port else {
        warn!(adapter = %adapter, "native zigbee serial port is not configured");
        set_status(
            &status,
            false,
            Some(format!("Native Zigbee adapter {adapter} selected, but no serial port is configured")),
            None,
        )
        .await;
        *lifecycle.write().await = DriverLifecycle::Failed(
            "Set ZIGBEE_SERIAL_PORT before using the native Zigbee backend".to_string(),
        );
        drain_pending_requests(
            &mut command_rx,
            AppError::service_unavailable("Set ZIGBEE_SERIAL_PORT before using the native Zigbee backend"),
        )
        .await;
        return;
    };

    let seed_devices: Vec<DiscoveredDevice> = known_devices.into_iter().map(seed_known_device).collect();

    // ---- Outer reconnect loop ----
    // On the first iteration `saved_devices` comes from the config.
    // On subsequent iterations it comes from the old context's device list
    // (which includes runtime-discovered state, desired brightness, etc.).
    let mut saved_devices = seed_devices;
    let mut reconnect_attempts: u32 = 0;

    'reconnect: loop {
        // --- Open EZSP context ---
        let context_result = match adapter.as_str() {
            "ember" => open_ezsp_context(&serial_port).await,
            other => Err(AppError::service_unavailable(format!(
                "Unsupported native Zigbee adapter: {other}"
            ))),
        };

        let mut context = match context_result {
            Ok(context) => context,
            Err(error) => {
                reconnect_attempts += 1;
                if reconnect_attempts > MAX_RECONNECT_ATTEMPTS {
                    error!(
                        adapter = %adapter,
                        serial_port = %serial_port,
                        attempts = reconnect_attempts,
                        error = %error,
                        "exhausted reconnect attempts — giving up"
                    );
                    set_status(
                        &status,
                        false,
                        Some(format!("Failed to open native Zigbee adapter {adapter} on {serial_port}")),
                        Some(error.to_string()),
                    )
                    .await;
                    *lifecycle.write().await = DriverLifecycle::Failed(error.to_string());
                    drain_pending_requests(&mut command_rx, error).await;
                    return;
                }
                warn!(
                    adapter = %adapter,
                    serial_port = %serial_port,
                    attempt = reconnect_attempts,
                    error = %error,
                    "failed to open EZSP context — retrying in {:?}",
                    RECONNECT_DELAY,
                );
                set_status(
                    &status,
                    false,
                    Some(format!("Reconnecting native Zigbee on {serial_port} (attempt {reconnect_attempts})")),
                    Some(error.to_string()),
                )
                .await;
                tokio::time::sleep(RECONNECT_DELAY).await;
                continue 'reconnect;
            }
        };

        context.joined_devices = saved_devices.clone();

        let network_state = match ensure_coordinator_network(&mut context, &serial_port).await {
            Ok(state) => state,
            Err(error) => {
                reconnect_attempts += 1;
                if reconnect_attempts > MAX_RECONNECT_ATTEMPTS {
                    error!(
                        serial_port = %serial_port,
                        attempts = reconnect_attempts,
                        error = %error,
                        "exhausted reconnect attempts during network init — giving up"
                    );
                    set_status(
                        &status,
                        false,
                        Some(format!("Failed to initialize native Zigbee network on {serial_port}")),
                        Some(error.to_string()),
                    )
                    .await;
                    *lifecycle.write().await = DriverLifecycle::Failed(error.to_string());
                    drain_pending_requests(&mut command_rx, error).await;
                    return;
                }
                warn!(
                    serial_port = %serial_port,
                    attempt = reconnect_attempts,
                    error = %error,
                    "network init failed — tearing down and retrying"
                );
                teardown_context(context).await;
                tokio::time::sleep(RECONNECT_DELAY).await;
                continue 'reconnect;
            }
        };

        // Successful init — reset reconnect counter.
        reconnect_attempts = 0;

        *driver_network_state.write().await = ember_network_state_to_driver_state(network_state);
        *lifecycle.write().await = DriverLifecycle::Ready;
        set_status(
            &status,
            true,
            Some(format!(
                "Native Zigbee connected on {serial_port} with network state {}",
                network_state_label(network_state)
            )),
            None,
        )
        .await;

        info!(adapter = %adapter, serial_port = %serial_port, "native zigbee EZSP stack initialized");

        // ---- Inner event loop ----
        let mut tick = interval(POLL_INTERVAL);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut tick_count: u32 = 0;

        let reconnect_reason: Option<String> = 'event_loop: loop {
            tokio::select! {
                maybe_request = command_rx.recv() => {
                    let Some(request) = maybe_request else {
                        // Channel closed — clean shutdown.
                        teardown_context(context).await;
                        return;
                    };

                    // Wrap the command in a timeout.  If it fires, the EZSP
                    // pipeline is assumed dead — we tear everything down and
                    // reconnect.  This is safe because we never reuse the
                    // pipeline after a timeout.
                    let result = match timeout(EZSP_COMMAND_TIMEOUT, handle_command(&mut context, request.command)).await {
                        Ok(inner_result) => inner_result,
                        Err(_elapsed) => {
                            error!(
                                serial_port = %serial_port,
                                "EZSP command timed out after {:?} — triggering reconnect",
                                EZSP_COMMAND_TIMEOUT,
                            );
                            let _ = request.reply_tx.send(Err(AppError::service_unavailable(
                                "EZSP command timed out — reconnecting pipeline",
                            )));
                            break 'event_loop Some(format!(
                                "EZSP command timed out after {:?}",
                                EZSP_COMMAND_TIMEOUT,
                            ));
                        }
                    };

                    match &result {
                        Ok(()) => {
                            context.last_activity = Instant::now();
                            sync_status_devices(&status, &context.joined_devices).await;
                        }
                        Err(error) => {
                            warn!(serial_port = %serial_port, error = %error, "native zigbee command failed");
                        }
                    }
                    if let Err(error) = request.reply_tx.send(result) {
                        warn!(error = ?error, "native zigbee command response receiver dropped");
                    }
                }
                _ = tick.tick() => {
                    tick_count = tick_count.wrapping_add(1);

                    // --- Health check: ASH tasks alive? ---
                    if let Some(reason) = check_pipeline_health(&context) {
                        break 'event_loop Some(reason.to_string());
                    }

                    // --- Drain callbacks ---
                    while let Ok(callback) = context.callbacks_rx.try_recv() {
                        context.last_activity = Instant::now();
                        if let Some(event) = handle_callback(&mut context, callback).await {
                            debug!(event = ?event, "native zigbee callback handled");
                            match event {
                                NativeZigbeeEvent::TransportReady => {
                                    set_status(&status, true, Some(format!("Native Zigbee transport connected on {serial_port} ({adapter})")), None).await;
                                }
                                NativeZigbeeEvent::NetworkState { status: network_status } => {
                                    *driver_network_state.write().await = match network_status.as_str() {
                                        "joined" => DriverNetworkState::Joined,
                                        "no-network" => DriverNetworkState::NoNetwork,
                                        _ => DriverNetworkState::Unknown,
                                    };
                                    set_status(&status, true, Some(format!("Native Zigbee network state: {network_status}")), None).await;
                                }
                                NativeZigbeeEvent::DeviceJoined { node_id, eui64 } => {
                                    set_status(&status, true, Some(format!("Native Zigbee device joined: {eui64} ({node_id:#06x})")), None).await;
                                    sync_status_devices(&status, &context.joined_devices).await;
                                }
                                NativeZigbeeEvent::DeviceAnnounced { node_id, eui64 } => {
                                    set_status(&status, true, Some(format!("Native Zigbee device announced: {eui64} ({node_id:#06x})")), None).await;
                                    sync_status_devices(&status, &context.joined_devices).await;
                                }
                                NativeZigbeeEvent::IncomingMessage { node_id, cluster_id, payload } => {
                                    debug!(node_id, cluster_id, payload = %hex_bytes(&payload), "native zigbee incoming message");
                                }
                            }
                        }
                    }

                    if tick_count % DISCOVERY_RETRY_INTERVAL_TICKS == 0 {
                        // Wrap interview retries in a timeout — a hung send_unicast here
                        // would block the entire event loop.
                        if timeout(EZSP_COMMAND_TIMEOUT, retry_pending_interviews(&mut context)).await.is_err() {
                            break 'event_loop Some("interview retry timed out".to_string());
                        }
                        sync_status_devices(&status, &context.joined_devices).await;
                    }

                    if tick_count % LIVENESS_PROBE_INTERVAL_TICKS == 0 {
                        expire_unreachable_devices(&mut context);
                        // Wrap liveness probes + restore in a generous timeout.
                        // Multiple send_unicast calls happen here (one per device),
                        // so we allow more time than a single command.
                        let probe_timeout = EZSP_COMMAND_TIMEOUT * context.joined_devices.len().max(1) as u32;
                        if timeout(probe_timeout, async {
                            run_liveness_probes(&mut context).await;
                            restore_desired_state(&mut context).await;
                        }).await.is_err() {
                            break 'event_loop Some("liveness probe cycle timed out".to_string());
                        }
                        sync_status_devices(&status, &context.joined_devices).await;
                    }
                }
            }
        };

        // --- Reconnect path ---
        if let Some(reason) = reconnect_reason {
            saved_devices = context.joined_devices.clone();
            reconnect_attempts += 1;

            if reconnect_attempts > MAX_RECONNECT_ATTEMPTS {
                error!(
                    serial_port = %serial_port,
                    reason = %reason,
                    attempts = reconnect_attempts,
                    "exhausted reconnect attempts — giving up"
                );
                teardown_context(context).await;
                *lifecycle.write().await = DriverLifecycle::Failed(format!("Pipeline died: {reason}"));
                set_status(
                    &status,
                    false,
                    Some(format!("Native Zigbee pipeline died on {serial_port}: {reason}")),
                    Some(reason),
                )
                .await;
                drain_pending_requests(
                    &mut command_rx,
                    AppError::service_unavailable("EZSP pipeline died and reconnect attempts exhausted"),
                )
                .await;
                return;
            }

            warn!(
                serial_port = %serial_port,
                reason = %reason,
                attempt = reconnect_attempts,
                "EZSP pipeline unhealthy — tearing down and reconnecting"
            );
            set_status(
                &status,
                false,
                Some(format!("Reconnecting native Zigbee on {serial_port}: {reason} (attempt {reconnect_attempts})")),
                Some(reason),
            )
            .await;
            *lifecycle.write().await = DriverLifecycle::Starting;

            teardown_context(context).await;
            tokio::time::sleep(RECONNECT_DELAY).await;
            // continue 'reconnect — the outer loop re-opens the context
        }
    }
}

async fn open_ezsp_context(serial_port: &str) -> Result<EzspContext, AppError> {
    let protocol_version = std::env::var("ZIGBEE_EZSP_PROTOCOL_VERSION")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(DEFAULT_EZSP_PROTOCOL_VERSION);
    let attempts = [
        (BaudRate::RstCts, FlowControl::None, "no-flow-control"),
        (BaudRate::XOnXOff, FlowControl::Software, "xon-xoff"),
        (BaudRate::RstCts, FlowControl::Hardware, "rst-cts"),
    ];

    let mut last_error = None;

    for (baud_rate, flow_control, label) in attempts {
        info!(serial_port = %serial_port, mode = %label, "opening EZSP serial transport");
        match try_open_ezsp_context(serial_port, baud_rate, flow_control, label, protocol_version).await {
            Ok(context) => return Ok(context),
            Err(error) => {
                warn!(serial_port = %serial_port, mode = %label, error = %error, "EZSP init attempt failed");
                last_error = Some(error);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        AppError::service_unavailable(format!("Unable to initialize EZSP on {serial_port}"))
    }))
}

async fn try_open_ezsp_context(
    serial_port: &str,
    baud_rate: BaudRate,
    flow_control: FlowControl,
    mode_label: &str,
    protocol_version: u8,
) -> Result<EzspContext, AppError> {
    let serial = open_ash_serial(serial_port, baud_rate, flow_control)
        .map_err(|error| AppError::service_unavailable(format!(
            "Unable to open Zigbee serial port {serial_port} in {mode_label} mode: {error}"
        )))?;

    let (payload_tx, payload_rx) = mpsc::unbounded_channel::<Payload>();
    let (callback_tx, callback_rx) = mpsc::unbounded_channel::<Callback>();
    let actor = AshActor::new(serial, payload_tx, EZSP_CHANNEL_SIZE)
        .map_err(|error| AppError::service_unavailable(format!(
            "Unable to create ASH actor for {serial_port} in {mode_label} mode: {error}"
        )))?;
    let (tasks, proxy) = actor.spawn();

    let mut uart = EzspUart::new(proxy, payload_rx, callback_tx, protocol_version, EZSP_CHANNEL_SIZE);
    info!(serial_port = %serial_port, mode = %mode_label, protocol_version, "initializing EZSP UART");
    timeout(EZSP_INIT_TIMEOUT, uart.init())
        .await
        .map_err(|_| AppError::service_unavailable(format!(
            "Timed out initializing EZSP on {serial_port} in {mode_label} mode"
        )))?
        .map_err(|error| AppError::service_unavailable(format!(
            "Unable to initialize EZSP on {serial_port} in {mode_label} mode: {error}"
        )))?;

    info!(serial_port = %serial_port, mode = %mode_label, "EZSP UART initialized");

    Ok(EzspContext {
        uart,
        ash_tasks: tasks,
        callbacks_rx: callback_rx,
        joined_devices: Vec::new(),
        next_global_sequence: 1,
        next_device_sequence: HashMap::new(),
        last_activity: Instant::now(),
    })
}

async fn wait_for_driver_ready(lifecycle: &Arc<RwLock<DriverLifecycle>>) -> Result<(), AppError> {
    for _ in 0..30 {
        match &*lifecycle.read().await {
            DriverLifecycle::Ready => return Ok(()),
            DriverLifecycle::Failed(message) => {
                return Err(AppError::service_unavailable(message.clone()));
            }
            DriverLifecycle::Starting => tokio::time::sleep(StdDuration::from_millis(200)).await,
        }
    }

    Err(AppError::service_unavailable(
        "Native Zigbee adapter initialization timed out",
    ))
}

async fn wait_for_joined_network(network_state: &Arc<RwLock<DriverNetworkState>>) -> Result<(), AppError> {
    for _ in 0..30 {
        match *network_state.read().await {
            DriverNetworkState::Joined => return Ok(()),
            DriverNetworkState::NoNetwork | DriverNetworkState::Unknown => {
                tokio::time::sleep(StdDuration::from_millis(200)).await;
            }
        }
    }

    Err(AppError::service_unavailable(
        "Native Zigbee network did not report a joined state in time",
    ))
}

async fn ensure_coordinator_network(
    context: &mut EzspContext,
    serial_port: &str,
) -> Result<EmberNetworkStatus, AppError> {
    configure_local_endpoint(context).await?;
    configure_stack(context).await?;

    if let Err(error) = context
        .uart
        .network_init(NetworkInitBitmask::PARENT_INFO_IN_TOKEN)
        .await
    {
        warn!(serial_port = %serial_port, error = %error, "ezsp network_init failed");
    }

    let mut state = context
        .uart
        .network_state()
        .await
        .map_err(map_ezsp_error("read network state"))?;

    if state == EmberNetworkStatus::NoNetwork {
        info!(serial_port = %serial_port, "forming new Zigbee coordinator network");
        form_coordinator_network(context).await?;
        state = wait_for_network_ready(context).await?;
    }

    log_network_parameters(context, serial_port).await?;

    Ok(state)
}

/// Gracefully tear down the EZSP pipeline.  Aborts the EZSP splitter task and
/// terminates the ASH transmitter/receiver tasks.  This ensures no orphaned
/// background tasks leak when we reconnect.
async fn teardown_context(context: EzspContext) {
    info!("tearing down EZSP pipeline");
    // First abort the EZSP splitter (consumes uart).
    context.uart.abort().await;
    // Then terminate ASH actor tasks (transmitter + receiver).
    if let Err(error) = context.ash_tasks.terminate().await {
        warn!(error = ?error, "ASH tasks termination returned an error (non-fatal)");
    }
    info!("EZSP pipeline torn down");
}

/// Check whether the EZSP pipeline is healthy.  Returns a human-readable reason
/// if the pipeline should be torn down and rebuilt.
fn check_pipeline_health(context: &EzspContext) -> Option<&'static str> {
    if !context.ash_tasks.is_alive() {
        return Some("ASH transport task(s) died");
    }
    if context.last_activity.elapsed() > WATCHDOG_TIMEOUT {
        return Some("EZSP watchdog timeout — no activity");
    }
    None
}

async fn configure_local_endpoint(context: &mut EzspContext) -> Result<(), AppError> {
    context
        .uart
        .add_endpoint(
            DEFAULT_SOURCE_ENDPOINT,
            HOME_AUTOMATION_PROFILE_ID,
            DEFAULT_HOME_GATEWAY_DEVICE_ID,
            0,
            DEFAULT_LOCAL_INPUT_CLUSTERS.iter().copied().collect(),
            DEFAULT_LOCAL_OUTPUT_CLUSTERS.iter().copied().collect(),
        )
        .await
        .map_err(map_ezsp_error("add local endpoint"))
}

async fn configure_stack(context: &mut EzspContext) -> Result<(), AppError> {
    context
        .uart
        .set_configuration_value(config::Id::StackProfile, DEFAULT_STACK_PROFILE)
        .await
        .map_err(map_ezsp_error("set stack profile"))?;
    context
        .uart
        .set_configuration_value(config::Id::SecurityLevel, DEFAULT_SECURITY_LEVEL)
        .await
        .map_err(map_ezsp_error("set security level"))?;
    context
        .uart
        .set_policy(
            policy::Id::MessageContentsInCallback,
            u8::from(decision::Id::MessageTagOnlyInCallback),
        )
        .await
        .map_err(map_ezsp_error("set message contents callback policy"))?;
    Ok(())
}

async fn form_coordinator_network(context: &mut EzspContext) -> Result<(), AppError> {
    let coordinator_eui64 = context
        .uart
        .get_eui64()
        .await
        .map_err(map_ezsp_error("get coordinator EUI64"))?;
    let channel = configured_network_channel();
    let tx_power = configured_network_tx_power();
    let pan_id = match configured_pan_id() {
        Some(value) => value,
        None => random_pan_id(
            context
                .uart
                .get_random_number()
                .await
                .map_err(map_ezsp_error("generate PAN ID"))?,
        ),
    };
    let extended_pan_id = configured_extended_pan_id().unwrap_or_else(|| derive_extended_pan_id(coordinator_eui64));

    info!(
        pan_id = format_args!("0x{pan_id:04x}"),
        extended_pan_id = %extended_pan_id,
        channel,
        tx_power,
        "forming Zigbee coordinator network"
    );

    context
        .uart
        .set_initial_security_state(build_initial_security_state())
        .await
        .map_err(map_ezsp_error("set initial security state"))?;

    context
        .uart
        .form_network(EmberNetworkParameters::new(
            extended_pan_id,
            pan_id,
            tx_power,
            channel,
            EmberJoinMethod::MacAssociation,
            0,
            0,
            1_u32 << channel,
        ))
        .await
        .map_err(map_ezsp_error("form coordinator network"))?;

    Ok(())
}

async fn log_network_parameters(context: &mut EzspContext, serial_port: &str) -> Result<(), AppError> {
    let (node_type, parameters) = context
        .uart
        .get_network_parameters()
        .await
        .map_err(map_ezsp_error("get network parameters"))?;
    info!(
        serial_port = %serial_port,
        node_type = %node_type,
        pan_id = format_args!("0x{:04x}", parameters.pan_id()),
        extended_pan_id = %parameters.extended_pan_id(),
        channel = parameters.radio_channel(),
        tx_power = parameters.radio_tx_power(),
        join_method = ?parameters.join_method(),
        "native zigbee network parameters"
    );
    Ok(())
}

async fn wait_for_network_ready(context: &mut EzspContext) -> Result<EmberNetworkStatus, AppError> {
    for _ in 0..25 {
        while let Ok(callback) = context.callbacks_rx.try_recv() {
            let _ = handle_callback(context, callback).await;
        }

        let state = context
            .uart
            .network_state()
            .await
            .map_err(map_ezsp_error("read network state after form"))?;
        if state != EmberNetworkStatus::NoNetwork && state != EmberNetworkStatus::JoiningNetwork {
            return Ok(state);
        }

        tokio::time::sleep(StdDuration::from_millis(200)).await;
    }

    Err(AppError::service_unavailable(
        "Timed out waiting for the Zigbee coordinator network to come up",
    ))
}

fn build_initial_security_state() -> initial::State {
    initial::State::new(
        initial::Bitmask::TRUST_CENTER_GLOBAL_LINK_KEY
            | initial::Bitmask::HAVE_PRECONFIGURED_KEY
            | initial::Bitmask::REQUIRE_ENCRYPTED_KEY,
        ZIGBEE_ALLIANCE09_LINK_KEY,
        [0; 16],
        0,
        Eui64::default(),
    )
}

fn configured_network_channel() -> u8 {
    parse_u8_env("ZIGBEE_CHANNEL")
        .filter(|value| (11..=26).contains(value))
        .unwrap_or(DEFAULT_NETWORK_CHANNEL)
}

fn configured_network_tx_power() -> u8 {
    parse_u8_env("ZIGBEE_TX_POWER").unwrap_or(DEFAULT_NETWORK_TX_POWER)
}

fn configured_pan_id() -> Option<u16> {
    parse_u16_env("ZIGBEE_PAN_ID").map(random_pan_id)
}

fn configured_extended_pan_id() -> Option<Eui64> {
    let value = std::env::var("ZIGBEE_EXTENDED_PAN_ID").ok()?;
    let compact = value.chars().filter(char::is_ascii_hexdigit).collect::<String>();
    if compact.len() != 16 {
        return None;
    }

    let mut bytes = [0_u8; 8];
    for (index, chunk_start) in (0..16).step_by(2).enumerate() {
        bytes[index] = u8::from_str_radix(&compact[chunk_start..chunk_start + 2], 16).ok()?;
    }

    Some(Eui64::from(bytes))
}

fn derive_extended_pan_id(coordinator_eui64: Eui64) -> Eui64 {
    let mut bytes = coordinator_eui64.into_array();
    bytes[0] ^= 0x02;
    Eui64::from(bytes)
}

fn random_pan_id(value: u16) -> u16 {
    match value {
        0x0000 | 0xffff => 0x1a62,
        other => other,
    }
}

fn parse_u8_env(name: &str) -> Option<u8> {
    let value = std::env::var(name).ok()?;
    parse_u16_literal(&value).ok()?.try_into().ok()
}

fn parse_u16_env(name: &str) -> Option<u16> {
    let value = std::env::var(name).ok()?;
    parse_u16_literal(&value).ok()
}

fn parse_u16_literal(value: &str) -> Result<u16, std::num::ParseIntError> {
    let trimmed = value.trim();
    if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16)
    } else {
        trimmed.parse::<u16>()
    }
}

async fn handle_command(context: &mut EzspContext, command: NativeZigbeeCommand) -> Result<(), AppError> {
    match command {
        NativeZigbeeCommand::PermitJoin { seconds } => {
            let duration = if seconds == 0 {
                NetworkDuration::Disable
            } else {
                NetworkDuration::try_from(StdDuration::from_secs(u64::from(seconds)))
                    .map_err(|error| AppError::service_unavailable(format!(
                        "Invalid permit-join duration {seconds}: {error}"
                    )))?
            };
            context
                .uart
                .permit_joining(duration)
                .await
                .map_err(map_ezsp_error("permit join"))?;
            info!(seconds, "native zigbee permit join updated");
            Ok(())
        }
        NativeZigbeeCommand::DiscoverDevices => {
            let targets = context.joined_devices.clone();
            info!(known_devices = targets.len(), "native zigbee discovery requested");
            if targets.is_empty() {
                debug!("native zigbee discovery skipped because no joined devices are known yet");
            }
            // NOTE: we intentionally do NOT wrap these EZSP calls in a timeout.
            // The EZSP UART protocol is strictly request-response: dropping a
            // `communicate()` future mid-`receive()` (as `timeout()` does) orphans
            // the NCP's response in the pipeline, permanently desynchronising all
            // subsequent commands.  If the NCP is slow, we wait.
            for target in targets {
                info!(node_id = format_args!("0x{:04x}", target.node_id), eui64 = %target.eui64, endpoint = ?target.endpoint, "probing known Zigbee device");
                if should_probe_active_endpoints(&target) {
                    match request_active_endpoints(context, target.node_id).await {
                        Ok(()) => {}
                        Err(error) => {
                            warn!(node_id = format_args!("0x{:04x}", target.node_id), eui64 = %target.eui64, error = %error, "native zigbee active endpoint probe failed");
                            continue;
                        }
                    }
                }
                if target.endpoint.is_some() {
                    if let Err(error) = refresh_device_state(context, &target).await {
                        warn!(node_id = format_args!("0x{:04x}", target.node_id), eui64 = %target.eui64, error = %error, "native zigbee state refresh failed during discovery");
                    }
                }
            }
            Ok(())
        }
        NativeZigbeeCommand::GetLampState { lamp_id } => {
            let target = find_target_device(context, &lamp_id)?;
            refresh_device_state(context, &target).await
        }
        NativeZigbeeCommand::SetPower { lamp_id, enabled } => {
            let target = find_target_device(context, &lamp_id)?;

            let endpoint = target
                .endpoint
                .ok_or_else(|| AppError::service_unavailable(format!(
                    "Lamp {lamp_id} has no discovered endpoint yet; run discovery first"
                )))?;
            if !target.input_clusters.contains(&ON_OFF_CLUSTER_ID) {
                return Err(AppError::service_unavailable(format!(
                    "Lamp {lamp_id} does not expose the On/Off cluster yet"
                )));
            }

            let aps_frame = EzspApsFrame::new(
                HOME_AUTOMATION_PROFILE_ID,
                ON_OFF_CLUSTER_ID,
                DEFAULT_SOURCE_ENDPOINT,
                endpoint,
                EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                0,
                0,
            );
            let sequence = next_device_sequence(context, target.node_id);
            let zcl_payload = build_on_off_command_payload(enabled, sequence);

            context
                .uart
                .send_unicast(
                    Destination::Direct(NodeId::from(target.node_id)),
                    aps_frame,
                    0,
                    zcl_payload.into_iter().collect(),
                )
                .await
                .map(|_| ())
                .map_err(map_ezsp_error("send unicast on/off"))?;

            if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == target.node_id) {
                device.is_on = enabled;
            }

            Ok(())
        }
        NativeZigbeeCommand::SetBrightness { lamp_id, brightness } => {
            let target = find_target_device(context, &lamp_id)?;

            if !target.supports_brightness {
                return Err(AppError::service_unavailable(format!(
                    "Native brightness is not available for {lamp_id}; the lamp capabilities are incomplete"
                )));
            }

            let endpoint = target
                .endpoint
                .ok_or_else(|| AppError::service_unavailable(format!(
                    "Lamp {lamp_id} has no discovered endpoint yet; run discovery first"
                )))?;
            let sequence = next_device_sequence(context, target.node_id);
            let zcl_payload = build_brightness_command_payload(brightness, sequence);
            let aps_frame = EzspApsFrame::new(
                HOME_AUTOMATION_PROFILE_ID,
                0x0008,
                DEFAULT_SOURCE_ENDPOINT,
                endpoint,
                EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                0,
                0,
            );

            context
                .uart
                .send_unicast(
                    Destination::Direct(NodeId::from(target.node_id)),
                    aps_frame,
                    0,
                    zcl_payload.into_iter().collect(),
                )
                .await
                .map(|_| ())
                .map_err(map_ezsp_error("send unicast brightness"))?;

            if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == target.node_id) {
                device.brightness = brightness.min(100);
                device.is_on = brightness > 0;
                device.desired_brightness = Some(brightness.min(100));
            }

            Ok(())
        }
        NativeZigbeeCommand::SetTemperature { lamp_id, temperature } => {
            let target = find_target_device(context, &lamp_id)?;

            if !target.supports_temperature {
                return Err(AppError::service_unavailable(format!(
                    "Native color temperature is not available for {lamp_id}; the lamp capabilities are incomplete"
                )));
            }

            let endpoint = target
                .endpoint
                .ok_or_else(|| AppError::service_unavailable(format!(
                    "Lamp {lamp_id} has no discovered endpoint yet; run discovery first"
                )))?;
            let sequence = next_device_sequence(context, target.node_id);
            let zcl_payload = build_color_temperature_command_payload(temperature, sequence);
            let aps_frame = EzspApsFrame::new(
                HOME_AUTOMATION_PROFILE_ID,
                0x0300,
                DEFAULT_SOURCE_ENDPOINT,
                endpoint,
                EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                0,
                0,
            );

            context
                .uart
                .send_unicast(
                    Destination::Direct(NodeId::from(target.node_id)),
                    aps_frame,
                    0,
                    zcl_payload.into_iter().collect(),
                )
                .await
                .map(|_| ())
                .map_err(map_ezsp_error("send unicast color temperature"))?;

            if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == target.node_id) {
                device.temperature = Some(temperature.min(100));
                device.desired_temperature = Some(temperature.min(100));
            }

            Ok(())
        }
    }
}

fn find_target_device(context: &EzspContext, lamp_id: &str) -> Result<DiscoveredDevice, AppError> {
    let normalized_lamp_id = normalize_device_id(lamp_id, 0);
    context
        .joined_devices
        .iter()
        .find(|device| {
            device.eui64 == lamp_id
                || normalize_device_id(&device.eui64, device.node_id) == normalized_lamp_id
                || format!("{:016x}", device.node_id) == normalized_lamp_id
        })
        .cloned()
        .ok_or_else(|| AppError::service_unavailable(format!(
            "No discovered native Zigbee lamp matches {lamp_id}; discovery is not implemented yet"
        )))
}

async fn refresh_device_state(context: &mut EzspContext, target: &DiscoveredDevice) -> Result<(), AppError> {
    let endpoint = target
        .endpoint
        .ok_or_else(|| AppError::service_unavailable(format!(
            "Lamp {} has no discovered endpoint yet; run discovery first",
            target.eui64
        )))?;

    send_read_attributes(context, target.node_id, endpoint, BASIC_CLUSTER_ID, &[0x0004, 0x0005]).await?;

    if target.input_clusters.contains(&ON_OFF_CLUSTER_ID) {
        send_read_attributes(context, target.node_id, endpoint, ON_OFF_CLUSTER_ID, &[0x0000]).await?;
    }
    if target.input_clusters.contains(&LEVEL_CONTROL_CLUSTER_ID) {
        send_read_attributes(context, target.node_id, endpoint, LEVEL_CONTROL_CLUSTER_ID, &[0x0000]).await?;
    }
    if target.has_color_control_cluster {
        send_read_attributes(context, target.node_id, endpoint, COLOR_CONTROL_CLUSTER_ID, &[0x0007]).await?;
    }

    Ok(())
}

async fn send_read_attributes(
    context: &mut EzspContext,
    node_id: u16,
    endpoint: u8,
    cluster_id: u16,
    attributes: &[u16],
) -> Result<(), AppError> {
    let sequence = next_device_sequence(context, node_id);
    let mut payload = vec![ZCL_GLOBAL_FRAME_CONTROL, sequence, ZCL_READ_ATTRIBUTES_COMMAND_ID];
    for attribute in attributes {
        payload.push((attribute & 0xff) as u8);
        payload.push((attribute >> 8) as u8);
    }
    let aps_frame = EzspApsFrame::new(
        HOME_AUTOMATION_PROFILE_ID,
        cluster_id,
        DEFAULT_SOURCE_ENDPOINT,
        endpoint,
        EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
        0,
        0,
    );

    context
        .uart
        .send_unicast(
            Destination::Direct(NodeId::from(node_id)),
            aps_frame,
            0,
            payload.into_iter().collect(),
        )
        .await
        .map(|_| ())
        .map_err(map_ezsp_error("send ZCL read attributes"))
}

async fn handle_callback(context: &mut EzspContext, callback: Callback) -> Option<NativeZigbeeEvent> {
    match callback {
        Callback::Networking(handler) => match handler {
            parameters::networking::handler::Handler::StackStatus(status) => {
                Some(NativeZigbeeEvent::NetworkState {
                    status: status
                        .result()
                        .map(network_state_from_stack_status)
                        .unwrap_or_else(|value| format!("unknown-stack-status-{value:#04x}")),
                })
            }
            parameters::networking::handler::Handler::ChildJoin(join) => {
                if join.joining() {
                    let eui64 = format_eui64(join.child_eui64());
                    let node_id: u16 = join.child_id().into();
                    if context.joined_devices.iter().all(|device| device.node_id != node_id) {
                        context.joined_devices.push(DiscoveredDevice {
                            node_id,
                            eui64: eui64.clone(),
                            endpoint: None,
                            input_clusters: Vec::new(),
                            output_clusters: Vec::new(),
                            supports_brightness: false,
                            supports_temperature: false,
                            has_color_control_cluster: false,
                            is_on: false,
                            brightness: 0,
                            temperature: None,
                            interview_completed: false,
                            model: None,
                            manufacturer: None,
                            connected: true,
                            reachable: true,
                            interview_attempts: 0,
                            last_seen: Some(Instant::now()),
                            desired_brightness: None,
                            desired_temperature: None,
                            desired_state_applied: true,
                        });
                    } else if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
                        device.connected = true;
                        device.reachable = true;
                        device.last_seen = Some(Instant::now());
                    }
                    request_known_device_discovery(context, node_id).await;
                    Some(NativeZigbeeEvent::DeviceJoined {
                        node_id,
                        eui64,
                    })
                } else {
                    None
                }
            }
            _ => None,
        },
        Callback::TrustCenter(handler) => match handler {
            parameters::trust_center::handler::Handler::TrustCenterJoin(join) => {
                handle_trust_center_join(context, join).await
            }
        },
        Callback::Messaging(handler) => match handler {
            parameters::messaging::handler::Handler::IncomingMessage(message) => {
                let node_id: u16 = message.sender().into();
                let cluster_id = message.aps_frame().cluster_id();
                let payload = message.message().to_vec();
                handle_incoming_cluster(context, node_id, cluster_id, &payload).await.or(Some(NativeZigbeeEvent::IncomingMessage {
                    node_id,
                    cluster_id,
                    payload,
                }))
            }
            _ => None,
        },
        _ => None,
    }
}

async fn handle_trust_center_join(
    context: &mut EzspContext,
    join: parameters::trust_center::handler::TrustCenterJoin,
) -> Option<NativeZigbeeEvent> {
    let status = join.status().ok()?;
    let node_id: u16 = join.new_node_id().into();
    let eui64 = format_eui64(join.new_node_eui64());

    match status {
        EmberDeviceUpdate::StandardSecuritySecuredRejoin
        | EmberDeviceUpdate::StandardSecurityUnsecuredJoin
        | EmberDeviceUpdate::StandardSecurityUnsecuredRejoin => {
            ensure_joined_device(context, node_id, eui64.clone());
            request_known_device_discovery(context, node_id).await;
            Some(NativeZigbeeEvent::DeviceJoined { node_id, eui64 })
        }
        EmberDeviceUpdate::DeviceLeft => {
            context.joined_devices.retain(|device| device.node_id != node_id && device.eui64 != eui64);
            None
        }
    }
}

fn ensure_joined_device(context: &mut EzspContext, node_id: u16, eui64: String) {
    if let Some(device) = context
        .joined_devices
        .iter_mut()
        .find(|device| device.eui64 == eui64 || device.node_id == node_id)
    {
        let was_reachable = device.reachable;
        device.node_id = node_id;
        device.eui64 = eui64;
        device.connected = true;
        device.reachable = true;
        device.last_seen = Some(Instant::now());
        if was_reachable {
            // The device was already reachable — this is an adapter self-healing
            // reconnect, not a physical power cycle.  Keep the previous is_on state
            // so that lamps that were off stay off.
        } else {
            // The device was unreachable (wall-switch / physical power cycle) —
            // the lamp has physically powered back on, so reflect that.
            device.is_on = true;
            if device.brightness == 0 {
                device.brightness = 100;
            }
        }
        if device.endpoint.is_none() {
            device.interview_attempts = 0;
        }
    } else {
        context.joined_devices.push(DiscoveredDevice {
            node_id,
            eui64,
            endpoint: None,
            input_clusters: Vec::new(),
            output_clusters: Vec::new(),
            supports_brightness: false,
            supports_temperature: false,
            has_color_control_cluster: false,
            is_on: true,
            brightness: 100,
            temperature: None,
            interview_completed: false,
            model: None,
            manufacturer: None,
            connected: true,
            reachable: true,
            interview_attempts: 0,
            last_seen: Some(Instant::now()),
            desired_brightness: None,
            desired_temperature: None,
            desired_state_applied: true,
        });
    }
}

/// Mark devices as unreachable if they haven't responded within [`LIVENESS_TIMEOUT`].
fn expire_unreachable_devices(context: &mut EzspContext) {
    let now = Instant::now();
    for device in &mut context.joined_devices {
        // Only expire interviewed devices that are currently considered reachable.
        if !device.reachable || device.endpoint.is_none() {
            continue;
        }

        let is_expired = device
            .last_seen
            .map_or(true, |last_seen| now.duration_since(last_seen) > LIVENESS_TIMEOUT);

        if is_expired {
            debug!(
                node_id = format_args!("0x{:04x}", device.node_id),
                eui64 = %device.eui64,
                "native zigbee device marked unreachable (no response within {:?})",
                LIVENESS_TIMEOUT,
            );
            device.reachable = false;
            // Mark desired state as unapplied so it will be re-sent when the device comes back.
            device.desired_state_applied = false;
        }
    }
}

/// Send a lightweight ZCL Read Attributes probe (On/Off attribute 0x0000) to all
/// interviewed devices that have the On/Off cluster — regardless of their current
/// `reachable` status, so that devices which come back online can be rediscovered.
///
/// **IMPORTANT**: probes are sent without any per-send timeout.  The EZSP UART protocol
/// is strictly request–response: `communicate()` calls `send()` then `receive()`.
/// If a `timeout()` fires while `receive()` is waiting, the future is dropped but the
/// NCP still delivers the response later.  That orphaned response permanently desynchronises
/// the EZSP pipeline, making every subsequent command hang.  Instead we let each
/// `send_unicast` take as long as the NCP needs; reachability is determined by
/// [`expire_unreachable_devices`] using the `last_seen` timestamp.
///
/// The ZCL response will be handled asynchronously via [`handle_incoming_cluster`] which
/// updates `last_seen` and `reachable`.
async fn run_liveness_probes(context: &mut EzspContext) {
    // Drain any callbacks that arrived since the last tick so that responses
    // from a previous probe cycle can update `last_seen` before we send new probes.
    drain_pending_callbacks(context).await;

    let targets: Vec<(u16, u8)> = context
        .joined_devices
        .iter()
        .filter(|device| device.endpoint.is_some() && device.interview_completed)
        .filter(|device| device.input_clusters.contains(&ON_OFF_CLUSTER_ID))
        .map(|device| (device.node_id, device.endpoint.unwrap()))
        .collect();

    for (node_id, endpoint) in targets {
        match send_read_attributes(context, node_id, endpoint, ON_OFF_CLUSTER_ID, &[0x0000]).await {
            Ok(()) => {
                debug!(
                    node_id = format_args!("0x{node_id:04x}"),
                    "liveness probe sent"
                );
            }
            Err(error) => {
                warn!(
                    node_id = format_args!("0x{node_id:04x}"),
                    error = %error,
                    "liveness probe send failed"
                );
            }
        }

        // Drain callbacks between probes so that responses update `last_seen`
        // promptly and don't pile up in the channel.
        drain_pending_callbacks(context).await;
    }
}

/// Drain all currently-queued callbacks without waiting,
/// so that pending ZCL responses can update device state immediately.
async fn drain_pending_callbacks(context: &mut EzspContext) {
    while let Ok(callback) = context.callbacks_rx.try_recv() {
        handle_callback(context, callback).await;
    }
}

/// Re-apply desired brightness and/or colour temperature to devices that have just
/// transitioned from unreachable to reachable (i.e. `reachable && !desired_state_applied`).
///
/// This handles the physical wall-switch scenario: a user turns off a lamp at the wall,
/// then turns it back on.  The lamp powers up at factory defaults, so we push the last
/// user-set brightness and temperature to it.
///
/// The lamp is intentionally forced ON when it reappears after a wall-switch (handled
/// elsewhere via `ensure_joined_device` when the device was previously unreachable),
/// so we do **not** touch the on/off state here.
///
/// **IMPORTANT**: no `timeout()` wrapper — see [`run_liveness_probes`] doc comment.
async fn restore_desired_state(context: &mut EzspContext) {
    // Collect targets: devices that are reachable, have an endpoint, and have unapplied desired state.
    let targets: Vec<(u16, u8, Option<u8>, Option<u8>, bool, bool)> = context
        .joined_devices
        .iter()
        .filter(|d| d.reachable && !d.desired_state_applied && d.endpoint.is_some())
        .filter(|d| d.desired_brightness.is_some() || d.desired_temperature.is_some())
        .map(|d| {
            (
                d.node_id,
                d.endpoint.unwrap(),
                d.desired_brightness,
                d.desired_temperature,
                d.supports_brightness,
                d.supports_temperature,
            )
        })
        .collect();

    for (node_id, endpoint, desired_brightness, desired_temperature, supports_brightness, supports_temperature) in targets {
        // Restore brightness
        if let Some(brightness) = desired_brightness {
            if supports_brightness {
                let sequence = next_device_sequence(context, node_id);
                let zcl_payload = build_brightness_command_payload(brightness, sequence);
                let aps_frame = EzspApsFrame::new(
                    HOME_AUTOMATION_PROFILE_ID,
                    LEVEL_CONTROL_CLUSTER_ID,
                    DEFAULT_SOURCE_ENDPOINT,
                    endpoint,
                    EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                    0,
                    0,
                );

                match context
                    .uart
                    .send_unicast(
                        Destination::Direct(NodeId::from(node_id)),
                        aps_frame,
                        0,
                        zcl_payload.into_iter().collect(),
                    )
                    .await
                {
                    Ok(_) => {
                        info!(
                            node_id = format_args!("0x{node_id:04x}"),
                            brightness,
                            "restored desired brightness after reconnect"
                        );
                        if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
                            device.brightness = brightness;
                            device.is_on = brightness > 0;
                        }
                    }
                    Err(error) => {
                        warn!(
                            node_id = format_args!("0x{node_id:04x}"),
                            error = %error,
                            "failed to restore desired brightness"
                        );
                        // Don't mark as applied — retry next cycle.
                        continue;
                    }
                }
            }
        }

        // Restore colour temperature
        if let Some(temperature) = desired_temperature {
            if supports_temperature {
                let sequence = next_device_sequence(context, node_id);
                let zcl_payload = build_color_temperature_command_payload(temperature, sequence);
                let aps_frame = EzspApsFrame::new(
                    HOME_AUTOMATION_PROFILE_ID,
                    COLOR_CONTROL_CLUSTER_ID,
                    DEFAULT_SOURCE_ENDPOINT,
                    endpoint,
                    EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
                    0,
                    0,
                );

                match context
                    .uart
                    .send_unicast(
                        Destination::Direct(NodeId::from(node_id)),
                        aps_frame,
                        0,
                        zcl_payload.into_iter().collect(),
                    )
                    .await
                {
                    Ok(_) => {
                        info!(
                            node_id = format_args!("0x{node_id:04x}"),
                            temperature,
                            "restored desired colour temperature after reconnect"
                        );
                        if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
                            device.temperature = Some(temperature);
                        }
                    }
                    Err(error) => {
                        warn!(
                            node_id = format_args!("0x{node_id:04x}"),
                            error = %error,
                            "failed to restore desired colour temperature"
                        );
                        // Don't mark as applied — retry next cycle.
                        continue;
                    }
                }
            }
        }

        // Both commands succeeded (or were skipped because unsupported) — mark as applied.
        if let Some(device) = context.joined_devices.iter_mut().find(|d| d.node_id == node_id) {
            device.desired_state_applied = true;
            info!(
                node_id = format_args!("0x{node_id:04x}"),
                eui64 = %device.eui64,
                "desired state fully restored after reconnect"
            );
        }

        // Drain callbacks so any responses update device state promptly.
        drain_pending_callbacks(context).await;
    }
}

async fn retry_pending_interviews(context: &mut EzspContext) {
    let retry_targets = context
        .joined_devices
        .iter()
        .filter(|device| device.connected && device.endpoint.is_none() && device.interview_attempts < 20)
        .map(|device| (device.node_id, device.eui64.clone(), device.interview_attempts))
        .collect::<Vec<_>>();

    for (node_id, eui64, attempts) in retry_targets {
        debug!(
            node_id = format_args!("0x{node_id:04x}"),
            eui64,
            attempts,
            "retrying native zigbee endpoint discovery"
        );
        if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
            device.interview_attempts = device.interview_attempts.saturating_add(1);
        }
        if let Err(error) = request_active_endpoints(context, node_id).await {
            warn!(node_id = format_args!("0x{node_id:04x}"), error = %error, "native zigbee endpoint discovery retry failed");
        }
    }
}

async fn request_active_endpoints(context: &mut EzspContext, node_id: u16) -> Result<(), AppError> {
    let sequence = next_device_sequence(context, node_id);
    let payload = vec![sequence, (node_id & 0xff) as u8, (node_id >> 8) as u8];
    let aps_frame = EzspApsFrame::new(
        ZDO_PROFILE_ID,
        ACTIVE_EP_REQ_CLUSTER_ID,
        0,
        0,
        EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
        0,
        0,
    );

    context
        .uart
        .send_unicast(
            Destination::Direct(NodeId::from(node_id)),
            aps_frame,
            0,
            payload.into_iter().collect(),
        )
        .await
        .map(|_| ())
        .map_err(map_ezsp_error("send Active_EP_req"))
}

async fn request_simple_descriptor(
    context: &mut EzspContext,
    node_id: u16,
    endpoint: u8,
) -> Result<(), AppError> {
    let sequence = next_device_sequence(context, node_id);
    let payload = vec![sequence, (node_id & 0xff) as u8, (node_id >> 8) as u8, endpoint];
    let aps_frame = EzspApsFrame::new(
        ZDO_PROFILE_ID,
        SIMPLE_DESC_REQ_CLUSTER_ID,
        0,
        0,
        EzspApsOptions::RETRY | EzspApsOptions::ENABLE_ROUTE_DISCOVERY,
        0,
        0,
    );

    context
        .uart
        .send_unicast(
            Destination::Direct(NodeId::from(node_id)),
            aps_frame,
            0,
            payload.into_iter().collect(),
        )
        .await
        .map(|_| ())
        .map_err(map_ezsp_error("send Simple_Desc_req"))
}

fn next_device_sequence(context: &mut EzspContext, node_id: u16) -> u8 {
    next_sequence_for_device(
        &mut context.next_device_sequence,
        &mut context.next_global_sequence,
        node_id,
    )
}

fn next_sequence_for_device(
    per_device: &mut HashMap<u16, u8>,
    next_global: &mut u8,
    node_id: u16,
) -> u8 {
    let current = per_device.entry(node_id).or_insert_with(|| {
        let value = *next_global;
        *next_global = next_global.wrapping_add(1);
        value
    });
    let sequence = *current;
    *current = current.wrapping_add(1);
    sequence
}

fn should_probe_active_endpoints(target: &DiscoveredDevice) -> bool {
    target.endpoint.is_none() || target.input_clusters.is_empty()
}

async fn request_known_device_discovery(context: &mut EzspContext, node_id: u16) {
    let target = context.joined_devices.iter().find(|device| device.node_id == node_id).cloned();
    let Some(target) = target else {
        return;
    };

    if should_probe_active_endpoints(&target) {
        let _ = request_active_endpoints(context, node_id).await;
    } else {
        let _ = refresh_device_state(context, &target).await;
    }
}

async fn handle_incoming_cluster(
    context: &mut EzspContext,
    node_id: u16,
    cluster_id: u16,
    payload: &[u8],
) -> Option<NativeZigbeeEvent> {
    match cluster_id {
        DEVICE_ANNCE_CLUSTER_ID => {
            if let Some(announcement) = parse_device_announce(payload) {
                ensure_joined_device(context, announcement.node_id, announcement.eui64.clone());
                request_known_device_discovery(context, announcement.node_id).await;
                Some(NativeZigbeeEvent::DeviceAnnounced {
                    node_id: announcement.node_id,
                    eui64: announcement.eui64,
                })
            } else {
                None
            }
        }
        ACTIVE_EP_RSP_CLUSTER_ID => {
            if let Some(endpoints) = parse_active_ep_response(payload) {
                let mut ordered_endpoints = endpoints;
                ordered_endpoints.sort_by_key(|endpoint| if *endpoint == 242 { 1 } else { 0 });
                for endpoint in ordered_endpoints {
                    let _ = request_simple_descriptor(context, node_id, endpoint).await;
                }
            }
            None
        }
        SIMPLE_DESC_RSP_CLUSTER_ID => {
            if let Some(description) = parse_simple_desc_response(payload) {
                debug!(
                    node_id = format_args!("0x{node_id:04x}"),
                    endpoint = description.endpoint,
                    profile_id = format_args!("0x{:04x}", description.profile_id),
                    device_id = format_args!("0x{:04x}", description.device_id),
                    input_clusters = ?description.input_clusters,
                    output_clusters = ?description.output_clusters,
                    "native zigbee simple descriptor parsed"
                );
                let mut refresh_target = None;
                if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
                    device.connected = true;
                    device.reachable = true;
                    device.last_seen = Some(Instant::now());
                    let should_replace_endpoint = (device.endpoint.is_none() && is_preferred_light_endpoint(&description))
                        || device.endpoint == Some(description.endpoint)
                        || (is_preferred_light_endpoint(&description)
                            && !device.input_clusters.contains(&ON_OFF_CLUSTER_ID)
                            && !device.input_clusters.contains(&LEVEL_CONTROL_CLUSTER_ID)
                            && !device.input_clusters.contains(&COLOR_CONTROL_CLUSTER_ID));

                    if should_replace_endpoint {
                        device.endpoint = Some(description.endpoint);
                        device.input_clusters = description.input_clusters.clone();
                        device.output_clusters = description.output_clusters.clone();
                        device.supports_brightness = description.input_clusters.contains(&LEVEL_CONTROL_CLUSTER_ID);
                        device.has_color_control_cluster = description.input_clusters.contains(&COLOR_CONTROL_CLUSTER_ID);
                        device.supports_temperature = device.supports_temperature && device.has_color_control_cluster;
                        device.interview_completed = true;
                        device.interview_attempts = 0;
                        refresh_target = Some(device.clone());
                    }
                }
                if let Some(target) = refresh_target {
                    let _ = refresh_device_state(context, &target).await;
                }
            }
            None
        }
        ON_OFF_CLUSTER_ID => {
            if payload.len() >= 8 && payload[2] == ZCL_READ_ATTRIBUTES_RESPONSE_COMMAND_ID {
                parse_zcl_read_attributes_response(context, node_id, cluster_id, payload);
            } else if payload.len() >= 3 && payload[2] <= 0x02 {
                if let Some(value) = payload.last().copied() {
                    if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
                        device.connected = true;
                        device.reachable = true;
                        device.last_seen = Some(Instant::now());
                        device.is_on = value != 0;
                    }
                }
            }
            None
        }
        LEVEL_CONTROL_CLUSTER_ID => {
            if payload.len() >= 8 && payload[2] == ZCL_READ_ATTRIBUTES_RESPONSE_COMMAND_ID {
                parse_zcl_read_attributes_response(context, node_id, cluster_id, payload);
            } else if payload.len() >= 3 && payload[2] == 0x04 {
                if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
                    device.connected = true;
                    device.reachable = true;
                    device.last_seen = Some(Instant::now());
                    if let Some(level) = payload.get(3).copied() {
                        device.brightness = ((u16::from(level) * 100) / 254) as u8;
                        device.is_on = level > 0;
                    }
                }
            }
            None
        }
        COLOR_CONTROL_CLUSTER_ID => {
            parse_zcl_read_attributes_response(context, node_id, cluster_id, payload);
            None
        }
        BASIC_CLUSTER_ID => {
            parse_zcl_read_attributes_response(context, node_id, cluster_id, payload);
            None
        }
        _ => None,
    }
}

fn parse_zcl_read_attributes_response(
    context: &mut EzspContext,
    node_id: u16,
    cluster_id: u16,
    payload: &[u8],
) {
    if payload.len() < 3 || payload[2] != ZCL_READ_ATTRIBUTES_RESPONSE_COMMAND_ID {
        return;
    }

    debug!(
        node_id = format_args!("0x{node_id:04x}"),
        cluster_id = format_args!("0x{cluster_id:04x}"),
        payload = %hex_bytes(payload),
        "native zigbee read attributes response"
    );

    let mut offset = 3;
    while offset + 4 <= payload.len() {
        let attribute_id = u16::from(payload[offset]) | (u16::from(payload[offset + 1]) << 8);
        offset += 2;
        let status = payload[offset];
        offset += 1;
        if status != 0 || offset >= payload.len() {
            continue;
        }

        let data_type = payload[offset];
        offset += 1;
        let Some((value_len, value_bytes)) = parse_zcl_attribute_value(data_type, &payload[offset..]) else {
            break;
        };
        offset += value_len;

        if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
            device.connected = true;
            device.reachable = true;
            device.last_seen = Some(Instant::now());
            match (cluster_id, attribute_id) {
                (ON_OFF_CLUSTER_ID, 0x0000) => {
                    if let Some(value) = value_bytes.first() {
                        device.is_on = *value != 0;
                    }
                }
                (LEVEL_CONTROL_CLUSTER_ID, 0x0000) => {
                    if let Some(value) = value_bytes.first() {
                        device.brightness = ((u16::from(*value) * 100) / 254) as u8;
                    }
                }
                (COLOR_CONTROL_CLUSTER_ID, 0x0007) if value_bytes.len() >= 2 => {
                    let raw = u16::from(value_bytes[0]) | (u16::from(value_bytes[1]) << 8);
                    let normalized = (((500_u16.saturating_sub(raw.min(500))) * 100) / (500 - 153)) as u8;
                    device.supports_temperature = true;
                    device.temperature = Some(normalized.min(100));
                }
                (BASIC_CLUSTER_ID, 0x0004) => {
                    if let Ok(text) = String::from_utf8(value_bytes.to_vec()) {
                        device.manufacturer = Some(text);
                    }
                }
                (BASIC_CLUSTER_ID, 0x0005) => {
                    if let Ok(text) = String::from_utf8(value_bytes.to_vec()) {
                        device.model = Some(text);
                    }
                }
                _ => {}
            }
        }
    }
}

fn parse_zcl_attribute_value(data_type: u8, payload: &[u8]) -> Option<(usize, &[u8])> {
    match data_type {
        0x10 | 0x18 | 0x20 => payload.first().map(|_| (1, &payload[..1])),
        0x21 => (payload.len() >= 2).then_some((2, &payload[..2])),
        0x42 => {
            let len = *payload.first()? as usize;
            (payload.len() > len).then_some((1 + len, &payload[1..1 + len]))
        }
        _ => None,
    }
}

fn parse_active_ep_response(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() < 5 {
        return None;
    }
    let status = payload[1];
    if status != 0 {
        return None;
    }
    let count = payload[4] as usize;
    if payload.len() < 5 + count {
        return None;
    }
    Some(payload[5..5 + count].to_vec())
}

struct DeviceAnnouncement {
    node_id: u16,
    eui64: String,
}

fn parse_device_announce(payload: &[u8]) -> Option<DeviceAnnouncement> {
    if payload.len() < 11 {
        return None;
    }

    let node_id = u16::from(payload[1]) | (u16::from(payload[2]) << 8);
    let mut eui64_bytes = [0_u8; 8];
    eui64_bytes.copy_from_slice(&payload[3..11]);

    Some(DeviceAnnouncement {
        node_id,
        eui64: format_eui64(Eui64::from(eui64_bytes)),
    })
}

fn build_on_off_command_payload(enabled: bool, sequence: u8) -> Vec<u8> {
    vec![
        ZCL_CLUSTER_COMMAND_FRAME_CONTROL,
        sequence,
        if enabled {
            ZCL_ON_OFF_COMMAND_ON
        } else {
            ZCL_ON_OFF_COMMAND_OFF
        },
    ]
}

fn brightness_percent_to_level(brightness: u8) -> u8 {
    ((u16::from(brightness.min(100)) * 254) / 100).max(1) as u8
}

fn build_brightness_command_payload(brightness: u8, sequence: u8) -> Vec<u8> {
    let level = brightness_percent_to_level(brightness);
    vec![
        ZCL_CLUSTER_COMMAND_FRAME_CONTROL,
        sequence,
        ZCL_LEVEL_CONTROL_COMMAND_MOVE_TO_LEVEL,
        level,
        0x00,
        0x00,
    ]
}

fn temperature_percent_to_mireds(temperature: u8) -> u16 {
    500_u16.saturating_sub((u16::from(temperature.min(100)) * (500 - 153)) / 100)
}

fn build_color_temperature_command_payload(temperature: u8, sequence: u8) -> Vec<u8> {
    let raw_temperature = temperature_percent_to_mireds(temperature);
    vec![
        ZCL_CLUSTER_COMMAND_FRAME_CONTROL,
        sequence,
        ZCL_COLOR_CONTROL_COMMAND_MOVE_TO_COLOR_TEMPERATURE,
        (raw_temperature & 0xff) as u8,
        (raw_temperature >> 8) as u8,
        0x00,
        0x00,
    ]
}

struct SimpleDescriptor {
    endpoint: u8,
    profile_id: u16,
    device_id: u16,
    input_clusters: Vec<u16>,
    output_clusters: Vec<u16>,
}

fn parse_simple_desc_response(payload: &[u8]) -> Option<SimpleDescriptor> {
    if payload.len() < 8 {
        return None;
    }
    let status = payload[1];
    if status != 0 {
        return None;
    }

    let descriptor_length = payload[4] as usize;
    if payload.len() < 5 + descriptor_length || descriptor_length < 8 {
        return None;
    }

    let descriptor = &payload[5..5 + descriptor_length];
    let endpoint = descriptor[0];
    let profile_id = u16::from(descriptor[1]) | (u16::from(descriptor[2]) << 8);
    let device_id = u16::from(descriptor[3]) | (u16::from(descriptor[4]) << 8);
    let mut offset = 6;
    if descriptor.len() <= offset {
        return None;
    }
    let input_count = descriptor[offset] as usize;
    offset += 1;
    let mut input_clusters = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        if descriptor.len() < offset + 2 {
            return None;
        }
        input_clusters.push(u16::from(descriptor[offset]) | (u16::from(descriptor[offset + 1]) << 8));
        offset += 2;
    }
    if descriptor.len() <= offset {
        return None;
    }
    let output_count = descriptor[offset] as usize;
    offset += 1;
    let mut output_clusters = Vec::with_capacity(output_count);
    for _ in 0..output_count {
        if descriptor.len() < offset + 2 {
            return None;
        }
        output_clusters.push(u16::from(descriptor[offset]) | (u16::from(descriptor[offset + 1]) << 8));
        offset += 2;
    }

    Some(SimpleDescriptor {
        endpoint,
        profile_id,
        device_id,
        input_clusters,
        output_clusters,
    })
}

fn is_preferred_light_endpoint(description: &SimpleDescriptor) -> bool {
    description.profile_id == HOME_AUTOMATION_PROFILE_ID
        && (description.input_clusters.contains(&ON_OFF_CLUSTER_ID)
            || description.input_clusters.contains(&LEVEL_CONTROL_CLUSTER_ID)
            || description.input_clusters.contains(&COLOR_CONTROL_CLUSTER_ID))
}

async fn drain_pending_requests(command_rx: &mut mpsc::Receiver<DriverRequest>, error: AppError) {
    let error_message = error.to_string();
    while let Some(request) = command_rx.recv().await {
        let _ = request
            .reply_tx
            .send(Err(AppError::service_unavailable(error_message.clone())));
    }
}

async fn set_status(
    status: &Arc<RwLock<NativeZigbeeStatus>>,
    connected: bool,
    message: Option<String>,
    last_error: Option<String>,
) {
    let mut guard = status.write().await;
    guard.connected = connected;
    guard.message = message;
    guard.last_error = last_error;
}

async fn sync_status_devices(
    status: &Arc<RwLock<NativeZigbeeStatus>>,
    devices: &[DiscoveredDevice],
) {
    let mut guard = status.write().await;
    guard.devices = devices
        .iter()
        .map(|device| NativeDiscoveredDevice {
            id: normalize_device_id(&device.eui64, device.node_id),
            node_id: device.node_id,
            eui64: device.eui64.clone(),
            endpoint: device.endpoint,
            input_clusters: device.input_clusters.clone(),
            output_clusters: device.output_clusters.clone(),
            supports_brightness: device.supports_brightness,
            supports_temperature: device.supports_temperature,
            connected: device.connected,
            reachable: device.reachable,
            is_on: device.is_on,
            brightness: device.brightness,
            temperature: device.temperature,
            model: device.model.clone(),
            manufacturer: device.manufacturer.clone(),
            last_seen: device.last_seen.map(|instant| {
                let elapsed = instant.elapsed();
                let wall_clock = Utc::now() - chrono::Duration::from_std(elapsed).unwrap_or_default();
                wall_clock.to_rfc3339()
            }),
        })
        .collect();
}

fn network_state_label(state: EmberNetworkStatus) -> &'static str {
    match state {
        EmberNetworkStatus::NoNetwork => "no-network",
        EmberNetworkStatus::JoiningNetwork => "joining",
        EmberNetworkStatus::JoinedNetwork => "joined",
        EmberNetworkStatus::JoinedNetworkNoParent => "joined-no-parent",
        EmberNetworkStatus::LeavingNetwork => "leaving",
    }
}

fn ember_network_state_to_driver_state(state: EmberNetworkStatus) -> DriverNetworkState {
    match state {
        EmberNetworkStatus::JoinedNetwork => DriverNetworkState::Joined,
        EmberNetworkStatus::JoiningNetwork
        | EmberNetworkStatus::NoNetwork
        | EmberNetworkStatus::JoinedNetworkNoParent
        | EmberNetworkStatus::LeavingNetwork => DriverNetworkState::NoNetwork,
    }
}

fn network_state_from_stack_status(status: ezsp::ember::Status) -> String {
    match status {
        ezsp::ember::Status::NetworkUp => "joined".to_string(),
        ezsp::ember::Status::NetworkDown => "down".to_string(),
        ezsp::ember::Status::NetworkOpened => "open-for-join".to_string(),
        ezsp::ember::Status::NetworkClosed => "join-closed".to_string(),
        other => other.to_string(),
    }
}

fn map_ezsp_error(context: &'static str) -> impl FnOnce(ezsp::Error) -> AppError {
    move |error| AppError::service_unavailable(format!("EZSP {context} failed: {error}"))
}

fn format_eui64(eui64: Eui64) -> String {
    eui64
        .into_array()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn seed_known_device(device: NativeKnownDevice) -> DiscoveredDevice {
    DiscoveredDevice {
        node_id: device.node_id,
        eui64: device.eui64,
        endpoint: device.endpoint,
        has_color_control_cluster: device.input_clusters.contains(&COLOR_CONTROL_CLUSTER_ID),
        input_clusters: device.input_clusters,
        output_clusters: device.output_clusters,
        supports_brightness: device.supports_brightness,
        supports_temperature: device.supports_temperature,
        is_on: false,
        brightness: 0,
        temperature: None,
        interview_completed: device.endpoint.is_some(),
        model: device.model,
        manufacturer: device.manufacturer,
        connected: false,
        reachable: false,
        interview_attempts: 0,
        last_seen: None,
        desired_brightness: None,
        desired_temperature: None,
        desired_state_applied: true,
    }
}

fn normalize_device_id(eui64: &str, node_id: u16) -> String {
    let normalized = eui64
        .chars()
        .filter(|character| character.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase();

    if normalized.is_empty() {
        format!("{:016x}", node_id)
    } else {
        normalized
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        ZCL_CLUSTER_COMMAND_FRAME_CONTROL, ZCL_COLOR_CONTROL_COMMAND_MOVE_TO_COLOR_TEMPERATURE,
        ZCL_LEVEL_CONTROL_COMMAND_MOVE_TO_LEVEL, ZCL_ON_OFF_COMMAND_OFF, ZCL_ON_OFF_COMMAND_ON,
        build_brightness_command_payload, build_color_temperature_command_payload,
        build_on_off_command_payload, brightness_percent_to_level, next_sequence_for_device,
        should_probe_active_endpoints, temperature_percent_to_mireds,
    };
    use std::collections::HashMap;

    #[test]
    fn on_off_payload_uses_cluster_command_frame() {
        assert_eq!(
            build_on_off_command_payload(false, 0x34),
            vec![ZCL_CLUSTER_COMMAND_FRAME_CONTROL, 0x34, ZCL_ON_OFF_COMMAND_OFF]
        );
        assert_eq!(
            build_on_off_command_payload(true, 0x35),
            vec![ZCL_CLUSTER_COMMAND_FRAME_CONTROL, 0x35, ZCL_ON_OFF_COMMAND_ON]
        );
    }

    #[test]
    fn brightness_payload_maps_percent_to_move_to_level() {
        assert_eq!(brightness_percent_to_level(0), 1);
        assert_eq!(brightness_percent_to_level(50), 127);
        assert_eq!(brightness_percent_to_level(100), 254);
        assert_eq!(
            build_brightness_command_payload(50, 0x22),
            vec![
                ZCL_CLUSTER_COMMAND_FRAME_CONTROL,
                0x22,
                ZCL_LEVEL_CONTROL_COMMAND_MOVE_TO_LEVEL,
                127,
                0x00,
                0x00,
            ]
        );
    }

    #[test]
    fn color_temperature_payload_maps_percent_to_mireds() {
        assert_eq!(temperature_percent_to_mireds(0), 500);
        assert_eq!(temperature_percent_to_mireds(100), 153);
        assert_eq!(temperature_percent_to_mireds(50), 327);
        assert_eq!(
            build_color_temperature_command_payload(50, 0x44),
            vec![
                ZCL_CLUSTER_COMMAND_FRAME_CONTROL,
                0x44,
                ZCL_COLOR_CONTROL_COMMAND_MOVE_TO_COLOR_TEMPERATURE,
                0x47,
                0x01,
                0x00,
                0x00,
            ]
        );
    }

    #[test]
    fn device_sequences_are_independent() {
        let mut per_device = HashMap::new();
        let mut next_global = 1;

        assert_eq!(next_sequence_for_device(&mut per_device, &mut next_global, 0x8a4c), 1);
        assert_eq!(next_sequence_for_device(&mut per_device, &mut next_global, 0x8a4c), 2);
        assert_eq!(next_sequence_for_device(&mut per_device, &mut next_global, 0x6cce), 2);
        assert_eq!(next_sequence_for_device(&mut per_device, &mut next_global, 0x8a4c), 3);
        assert_eq!(next_sequence_for_device(&mut per_device, &mut next_global, 0x6cce), 3);
    }

    #[test]
    fn startup_probe_only_runs_for_uninterviewed_devices() {
        let base = super::DiscoveredDevice {
            node_id: 0x8a4c,
            eui64: "ab:1e:d2:06:01:88:17:00".to_string(),
            endpoint: Some(11),
            input_clusters: vec![0, 3, 4, 5, 6, 8],
            output_clusters: vec![25],
            supports_brightness: true,
            supports_temperature: false,
            has_color_control_cluster: false,
            is_on: true,
            brightness: 100,
            temperature: None,
            interview_completed: true,
            model: Some("LWA001".to_string()),
            manufacturer: Some("Signify Netherlands B.V.".to_string()),
            connected: true,
            reachable: true,
            interview_attempts: 0,
            last_seen: None,
            desired_brightness: None,
            desired_temperature: None,
            desired_state_applied: true,
        };

        assert!(!should_probe_active_endpoints(&base));

        let mut missing_endpoint = base.clone();
        missing_endpoint.endpoint = None;
        assert!(should_probe_active_endpoints(&missing_endpoint));

        let mut missing_clusters = base;
        missing_clusters.input_clusters.clear();
        assert!(should_probe_active_endpoints(&missing_clusters));
    }

    #[test]
    fn announce_reuses_known_endpoint_without_reprobe() {
        let known = super::DiscoveredDevice {
            node_id: 0x2e34,
            eui64: "4b:8e:c6:08:01:88:17:00".to_string(),
            endpoint: Some(11),
            input_clusters: vec![0, 3, 4, 5, 6, 8],
            output_clusters: vec![25],
            supports_brightness: true,
            supports_temperature: false,
            has_color_control_cluster: false,
            is_on: true,
            brightness: 100,
            temperature: None,
            interview_completed: true,
            model: Some("LTG002".to_string()),
            manufacturer: Some("Signify Netherlands B.V.".to_string()),
            connected: true,
            reachable: true,
            interview_attempts: 0,
            last_seen: None,
            desired_brightness: None,
            desired_temperature: None,
            desired_state_applied: true,
        };

        assert!(!should_probe_active_endpoints(&known));
    }

    #[test]
    fn color_control_cluster_does_not_imply_temperature_support() {
        let mut device = super::DiscoveredDevice {
            node_id: 0x2e34,
            eui64: "4b:8e:c6:08:01:88:17:00".to_string(),
            endpoint: Some(11),
            input_clusters: vec![0, 3, 4, 5, 6, 8, super::COLOR_CONTROL_CLUSTER_ID],
            output_clusters: vec![25],
            supports_brightness: true,
            supports_temperature: false,
            has_color_control_cluster: true,
            is_on: true,
            brightness: 100,
            temperature: None,
            interview_completed: true,
            model: Some("LTG002".to_string()),
            manufacturer: Some("Signify Netherlands B.V.".to_string()),
            connected: true,
            reachable: true,
            interview_attempts: 0,
            last_seen: None,
            desired_brightness: None,
            desired_temperature: None,
            desired_state_applied: true,
        };

        device.supports_temperature = device.supports_temperature && device.has_color_control_cluster;

        assert!(device.has_color_control_cluster);
        assert!(!device.supports_temperature);
    }
}
