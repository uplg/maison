use std::{process::{Command, Stdio}, thread, time::{Duration, Instant}};

use ashv2::{Actor as AshActor, BaudRate, FlowControl, Payload, open as open_ash_serial};
use ezsp::{Ezsp, uart::Uart as EzspUart};
use tokio::{sync::mpsc, time::timeout};

const CHANNEL_SIZE: usize = 64;
const INIT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_EZSP_PROTOCOL_VERSION: u8 = 13;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    if std::env::var("ZIGBEE_PROBE_CHILD").ok().as_deref() == Some("1") {
        let serial_port = std::env::var("ZIGBEE_SERIAL_PORT")
            .map_err(|_| "ZIGBEE_SERIAL_PORT is required".to_string())?;
        let mode = std::env::var("ZIGBEE_PROBE_MODE")
            .map_err(|_| "ZIGBEE_PROBE_MODE is required in child mode".to_string())?;
        let protocol_version = std::env::var("ZIGBEE_EZSP_PROTOCOL_VERSION")
            .ok()
            .and_then(|value| value.parse::<u8>().ok())
            .unwrap_or(DEFAULT_EZSP_PROTOCOL_VERSION);

        let (baud_rate, flow_control) = match mode.as_str() {
            "no-flow-control" => (BaudRate::RstCts, FlowControl::None),
            "rst-cts" => (BaudRate::RstCts, FlowControl::Hardware),
            "xon-xoff" => (BaudRate::XOnXOff, FlowControl::Software),
            other => return Err(format!("Unsupported child probe mode: {other}")),
        };

        return probe_once(&serial_port, baud_rate, flow_control, &mode, protocol_version).await;
    }

    let serial_port = std::env::var("ZIGBEE_SERIAL_PORT")
        .map_err(|_| "ZIGBEE_SERIAL_PORT is required".to_string())?;
    let requested_mode = std::env::var("ZIGBEE_PROBE_MODE").ok();
    let protocol_version = std::env::var("ZIGBEE_EZSP_PROTOCOL_VERSION")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .unwrap_or(DEFAULT_EZSP_PROTOCOL_VERSION);

    println!("Probing EZSP on {serial_port}");

    let attempts = [
        (BaudRate::RstCts, FlowControl::None, "no-flow-control"),
        (BaudRate::XOnXOff, FlowControl::Software, "xon-xoff"),
        (BaudRate::RstCts, FlowControl::Hardware, "rst-cts"),
    ];

    for (baud_rate, flow_control, label) in attempts {
        let _ = (baud_rate, flow_control);
        if let Some(mode) = &requested_mode {
            if mode != label {
                continue;
            }
        }
        println!("- opening serial port in {label} mode");
        match probe_once_subprocess(&serial_port, label, protocol_version)? {
            Ok(()) => {
                println!("- success in {label} mode");
                return Ok(());
            }
            Err(error) => {
                println!("- failed in {label} mode: {error}");
            }
        }
    }

    if let Some(mode) = requested_mode {
        return Err(format!("Requested EZSP probe mode failed: {mode}"));
    }

    Err("All EZSP probe attempts failed".to_string())
}

fn probe_once_subprocess(serial_port: &str, mode: &str, protocol_version: u8) -> Result<Result<(), String>, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Unable to resolve current executable: {error}"))?;

    let mut child = Command::new(current_exe)
        .env("ZIGBEE_PROBE_CHILD", "1")
        .env("ZIGBEE_SERIAL_PORT", serial_port)
        .env("ZIGBEE_PROBE_MODE", mode)
        .env("ZIGBEE_EZSP_PROTOCOL_VERSION", protocol_version.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("Unable to spawn child probe for {mode}: {error}"))?;

    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(Ok(()))
                } else {
                    Ok(Err(format!("child probe exited with status {status}")))
                };
            }
            Ok(None) => {
                if started_at.elapsed() >= INIT_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(Err(format!("init timeout in {mode} mode after {:?}", INIT_TIMEOUT)));
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                return Err(format!("Unable to wait on child probe for {mode}: {error}"));
            }
        }
    }
}

async fn probe_once(
    serial_port: &str,
    baud_rate: BaudRate,
    flow_control: FlowControl,
    label: &str,
    protocol_version: u8,
) -> Result<(), String> {
    let serial = open_ash_serial(serial_port, baud_rate, flow_control)
        .map_err(|error| format!("open serial failed ({label}): {error}"))?;

    let (payload_tx, payload_rx) = mpsc::unbounded_channel::<Payload>();
    let (callback_tx, _callback_rx) = mpsc::unbounded_channel();

    println!("  creating ASH actor");
    let actor = AshActor::new(serial, payload_tx, CHANNEL_SIZE)
        .map_err(|error| format!("create ASH actor failed ({label}): {error}"))?;

    println!("  spawning ASH actor");
    let (_tasks, proxy) = actor.spawn();

    println!("  building EZSP UART");
    let mut uart = EzspUart::new(proxy, payload_rx, callback_tx, protocol_version, CHANNEL_SIZE);

    println!("  initializing EZSP v0x{protocol_version:02x} (timeout {:?})", INIT_TIMEOUT);
    let init_result = timeout(INIT_TIMEOUT, uart.init())
        .await
        .map_err(|_| format!("init timeout in {label} mode after {:?}", INIT_TIMEOUT))?;

    match init_result {
        Ok(version) => {
            println!("  init ok, negotiated version=0x{:02x}", version.protocol_version());
            Ok(())
        }
        Err(error) => Err(format!("init failed in {label} mode: {error}")),
    }
}
