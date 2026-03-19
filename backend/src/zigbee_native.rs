use std::{sync::Arc, time::Duration as StdDuration};

use ashv2::{Actor as AshActor, BaudRate, FlowControl, Payload, open as open_ash_serial};
use ezsp::{
    Callback, Ezsp, Messaging, Networking,
    ember::{NodeId, aps::{Frame as EzspApsFrame, Options as EzspApsOptions}, message::Destination, network::{Duration as NetworkDuration, Status as EmberNetworkStatus}},
    ezsp::network::InitBitmask as NetworkInitBitmask,
    parameters,
    uart::Uart as EzspUart,
};
use tokio::{
    sync::{Mutex, RwLock, mpsc, oneshot},
    task::JoinHandle,
    time::{MissedTickBehavior, interval, timeout},
};
use tracing::{debug, info, warn};

use crate::error::AppError;

const DEFAULT_EZSP_PROTOCOL_VERSION: u8 = 13;
const EZSP_CHANNEL_SIZE: usize = 64;
const EZSP_INIT_TIMEOUT: StdDuration = StdDuration::from_secs(5);
const POLL_INTERVAL: StdDuration = StdDuration::from_millis(200);
const ZDO_PROFILE_ID: u16 = 0x0000;
const ZCL_GLOBAL_FRAME_CONTROL: u8 = 0x00;
const ZCL_READ_ATTRIBUTES_COMMAND_ID: u8 = 0x00;
const ZCL_READ_ATTRIBUTES_RESPONSE_COMMAND_ID: u8 = 0x01;
const BASIC_CLUSTER_ID: u16 = 0x0000;
const HOME_AUTOMATION_PROFILE_ID: u16 = 0x0104;
const SIMPLE_DESC_REQ_CLUSTER_ID: u16 = 0x0004;
const ACTIVE_EP_REQ_CLUSTER_ID: u16 = 0x0005;
const SIMPLE_DESC_RSP_CLUSTER_ID: u16 = 0x8004;
const ACTIVE_EP_RSP_CLUSTER_ID: u16 = 0x8005;
const ON_OFF_CLUSTER_ID: u16 = 0x0006;
const LEVEL_CONTROL_CLUSTER_ID: u16 = 0x0008;
const COLOR_CONTROL_CLUSTER_ID: u16 = 0x0300;
const DEFAULT_SOURCE_ENDPOINT: u8 = 1;

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
}

#[derive(Debug, Default)]
struct NativeZigbeeStatus {
    connected: bool,
    message: Option<String>,
    last_error: Option<String>,
    devices: Vec<NativeDiscoveredDevice>,
}

struct DriverRequest {
    command: NativeZigbeeCommand,
    reply_tx: oneshot::Sender<Result<(), AppError>>,
}

enum DriverLifecycle {
    Starting,
    Ready,
    Failed(String),
}

#[derive(Clone)]
pub struct NativeZigbeeRuntime {
    status: Arc<RwLock<NativeZigbeeStatus>>,
    command_tx: mpsc::Sender<DriverRequest>,
    command_rx: Arc<Mutex<Option<mpsc::Receiver<DriverRequest>>>>,
    task: Arc<std::sync::Mutex<Option<JoinHandle<()>>>>,
    init_once: Arc<Mutex<bool>>,
    lifecycle: Arc<RwLock<DriverLifecycle>>,
    adapter: Arc<String>,
    serial_port: Arc<Option<String>>,
}

impl NativeZigbeeRuntime {
    pub fn spawn(adapter: String, serial_port: Option<String>) -> Self {
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
            adapter: Arc::new(adapter_label),
            serial_port: Arc::new(serial_port_label),
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

    pub async fn ensure_initialized(&self) {
        self.start_task_if_needed().await;

        let mut guard = self.init_once.lock().await;
        if *guard {
            return;
        }
        *guard = true;

        if let Err(error) = self.send(NativeZigbeeCommand::DiscoverDevices).await {
            warn!(adapter = %self.adapter, serial_port = ?self.serial_port, error = %error, "native zigbee lazy initialization failed");
            let mut status = self.status.write().await;
            status.connected = false;
            status.last_error = Some(error.to_string());
            status.message = Some("Native Zigbee lazy initialization failed".to_string());
        }
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
        let task_status = Arc::clone(&self.status);
        let lifecycle = Arc::clone(&self.lifecycle);

        let task = tokio::spawn(async move {
            run_native_driver(adapter, serial_port, task_status, lifecycle, command_rx).await;
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
    callbacks_rx: mpsc::Receiver<Callback>,
    joined_devices: Vec<DiscoveredDevice>,
    next_zdo_sequence: u8,
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
    is_on: bool,
    brightness: u8,
    temperature: Option<u8>,
    interview_completed: bool,
    model: Option<String>,
    manufacturer: Option<String>,
}

async fn run_native_driver(
    adapter: String,
    serial_port: Option<String>,
    status: Arc<RwLock<NativeZigbeeStatus>>,
    lifecycle: Arc<RwLock<DriverLifecycle>>,
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

    let context_result = match adapter.as_str() {
        "ember" => open_ezsp_context(&serial_port).await,
        other => Err(AppError::service_unavailable(format!(
            "Unsupported native Zigbee adapter: {other}"
        ))),
    };

    let mut context = match context_result {
        Ok(context) => context,
        Err(error) => {
            warn!(adapter = %adapter, serial_port = %serial_port, error = %error, "failed to open native zigbee adapter");
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
    };

    if let Err(error) = context
        .uart
        .network_init(NetworkInitBitmask::PARENT_INFO_IN_TOKEN)
        .await
    {
        warn!(serial_port = %serial_port, error = %error, "ezsp network_init failed");
    }

    match context.uart.network_state().await {
        Ok(state) => {
            *lifecycle.write().await = DriverLifecycle::Ready;
            set_status(
                &status,
                true,
                Some(format!("Native Zigbee connected on {serial_port} with network state {}", network_state_label(state))),
                None,
            )
            .await;
        }
        Err(error) => {
            warn!(serial_port = %serial_port, error = %error, "ezsp network_state failed");
            *lifecycle.write().await = DriverLifecycle::Ready;
            set_status(
                &status,
                true,
                Some(format!("Native Zigbee connected on {serial_port}, but network state is unknown")),
                Some(error.to_string()),
            )
            .await;
        }
    }

    info!(adapter = %adapter, serial_port = %serial_port, "native zigbee ezsp stack initialized");

    let mut tick = interval(POLL_INTERVAL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            maybe_request = command_rx.recv() => {
                let Some(request) = maybe_request else {
                    break;
                };
                let result = handle_command(&mut context, request.command).await;
                if let Err(error) = &result {
                    warn!(serial_port = %serial_port, error = %error, "native zigbee command failed");
                }
                if let Err(error) = request.reply_tx.send(result) {
                    warn!(error = ?error, "native zigbee command response receiver dropped");
                }
            }
            _ = tick.tick() => {
                while let Ok(callback) = context.callbacks_rx.try_recv() {
                    if let Some(event) = handle_callback(&mut context, callback).await {
                        debug!(event = ?event, "native zigbee callback handled");
                        match event {
                            NativeZigbeeEvent::TransportReady => {
                                set_status(&status, true, Some(format!("Native Zigbee transport connected on {serial_port} ({adapter})")), None).await;
                            }
                            NativeZigbeeEvent::NetworkState { status: network_status } => {
                                set_status(&status, true, Some(format!("Native Zigbee network state: {network_status}")), None).await;
                            }
                            NativeZigbeeEvent::DeviceJoined { node_id, eui64 } => {
                                set_status(&status, true, Some(format!("Native Zigbee device joined: {eui64} ({node_id:#06x})")), None).await;
                            }
                            NativeZigbeeEvent::IncomingMessage { node_id, cluster_id, payload } => {
                                debug!(node_id, cluster_id, payload = %hex_bytes(&payload), "native zigbee incoming message");
                            }
                        }
                        sync_status_devices(&status, &context.joined_devices).await;
                    }
                }
            }
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

    let (payload_tx, payload_rx) = mpsc::channel::<Payload>(EZSP_CHANNEL_SIZE);
    let (callback_tx, callback_rx) = mpsc::channel::<Callback>(EZSP_CHANNEL_SIZE);
    let actor = AshActor::new(serial, payload_tx, EZSP_CHANNEL_SIZE)
        .map_err(|error| AppError::service_unavailable(format!(
            "Unable to create ASH actor for {serial_port} in {mode_label} mode: {error}"
        )))?;
    let (tasks, proxy) = actor.spawn();
    let _ = tasks;

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
        callbacks_rx: callback_rx,
        joined_devices: Vec::new(),
        next_zdo_sequence: 1,
    })
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
                .map_err(map_ezsp_error("permit join"))
        }
        NativeZigbeeCommand::DiscoverDevices => {
            info!("native zigbee discovery requested");
            let node_ids = context
                .joined_devices
                .iter()
                .map(|device| device.node_id)
                .collect::<Vec<_>>();
            if node_ids.is_empty() {
                debug!("native zigbee discovery skipped because no joined devices are known yet");
            }
            for node_id in node_ids {
                request_active_endpoints(context, node_id).await?;
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
            let zcl_payload = vec![0x01_u8, 0x01_u8, if enabled { 0x01_u8 } else { 0x00_u8 }];

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

            refresh_device_state(context, &target).await
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
            let level = ((u16::from(brightness.min(100)) * 254) / 100).max(1) as u8;
            let zcl_payload = vec![0x01_u8, 0x04_u8, level, 0x00_u8, 0x00_u8];
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
            }

            refresh_device_state(context, &target).await
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
            let raw_temperature = 500_u16.saturating_sub((u16::from(temperature.min(100)) * (500 - 153)) / 100);
            let zcl_payload = vec![0x01_u8, 0x0a_u8, (raw_temperature & 0xff) as u8, (raw_temperature >> 8) as u8, 0x00_u8, 0x00_u8];
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
            }

            refresh_device_state(context, &target).await
        }
    }
}

fn find_target_device(context: &EzspContext, lamp_id: &str) -> Result<DiscoveredDevice, AppError> {
    context
        .joined_devices
        .iter()
        .find(|device| device.eui64 == lamp_id || format!("{:016x}", device.node_id) == lamp_id)
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
    if target.input_clusters.contains(&COLOR_CONTROL_CLUSTER_ID) {
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
    let sequence = next_zdo_sequence(context);
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
            sequence,
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
                    let eui64 = format!("{:?}", join.child_eui64());
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
                            is_on: false,
                            brightness: 0,
                            temperature: None,
                            interview_completed: false,
                            model: None,
                            manufacturer: None,
                        });
                    }
                    let _ = request_active_endpoints(context, node_id).await;
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
        Callback::Messaging(handler) => match handler {
            parameters::messaging::handler::Handler::IncomingMessage(message) => {
                let node_id: u16 = message.sender().into();
                let cluster_id = message.aps_frame().cluster_id();
                let payload = message.message().to_vec();
                handle_incoming_cluster(context, node_id, cluster_id, &payload).await;
                Some(NativeZigbeeEvent::IncomingMessage {
                    node_id,
                    cluster_id,
                    payload,
                })
            }
            _ => None,
        },
        _ => None,
    }
}

async fn request_active_endpoints(context: &mut EzspContext, node_id: u16) -> Result<(), AppError> {
    let sequence = next_zdo_sequence(context);
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
            sequence,
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
    let sequence = next_zdo_sequence(context);
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
            sequence,
            payload.into_iter().collect(),
        )
        .await
        .map(|_| ())
        .map_err(map_ezsp_error("send Simple_Desc_req"))
}

fn next_zdo_sequence(context: &mut EzspContext) -> u8 {
    let current = context.next_zdo_sequence;
    context.next_zdo_sequence = context.next_zdo_sequence.wrapping_add(1);
    current
}

async fn handle_incoming_cluster(
    context: &mut EzspContext,
    node_id: u16,
    cluster_id: u16,
    payload: &[u8],
) {
    match cluster_id {
        ACTIVE_EP_RSP_CLUSTER_ID => {
            if let Some(endpoints) = parse_active_ep_response(payload) {
                for endpoint in endpoints {
                    let _ = request_simple_descriptor(context, node_id, endpoint).await;
                }
            }
        }
        SIMPLE_DESC_RSP_CLUSTER_ID => {
            if let Some(description) = parse_simple_desc_response(payload) {
                if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
                    device.endpoint = Some(description.endpoint);
                    device.input_clusters = description.input_clusters.clone();
                    device.output_clusters = description.output_clusters.clone();
                    device.supports_brightness = description.input_clusters.contains(&LEVEL_CONTROL_CLUSTER_ID);
                    device.supports_temperature = description.input_clusters.contains(&COLOR_CONTROL_CLUSTER_ID);
                    device.interview_completed = true;
                }
            }
        }
        ON_OFF_CLUSTER_ID => {
            if let Some(value) = payload.last().copied() {
                if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
                    device.is_on = value != 0;
                }
            }
        }
        LEVEL_CONTROL_CLUSTER_ID => {
            if let Some(value) = payload.last().copied() {
                if let Some(device) = context.joined_devices.iter_mut().find(|device| device.node_id == node_id) {
                    device.brightness = ((u16::from(value) * 100) / 254) as u8;
                }
            }
        }
        COLOR_CONTROL_CLUSTER_ID => {
            parse_zcl_read_attributes_response(context, node_id, cluster_id, payload);
        }
        BASIC_CLUSTER_ID => {
            parse_zcl_read_attributes_response(context, node_id, cluster_id, payload);
        }
        _ => {}
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

struct SimpleDescriptor {
    endpoint: u8,
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
    let mut offset = 5;
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
        input_clusters,
        output_clusters,
    })
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
            id: format!("{:016x}", device.node_id),
            node_id: device.node_id,
            eui64: device.eui64.clone(),
            endpoint: device.endpoint,
            input_clusters: device.input_clusters.clone(),
            output_clusters: device.output_clusters.clone(),
            supports_brightness: device.supports_brightness,
            supports_temperature: device.supports_temperature,
            connected: true,
            reachable: true,
            is_on: device.is_on,
            brightness: device.brightness,
            temperature: device.temperature,
            model: device.model.clone(),
            manufacturer: device.manufacturer.clone(),
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

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
