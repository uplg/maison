use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, timeout};
use tokio_serial::{DataBits, FlowControl, Parity, SerialPortBuilderExt, StopBits};

const READ_TIMEOUT: Duration = Duration::from_millis(750);
const WRITE_TIMEOUT: Duration = Duration::from_millis(750);
const BETWEEN_PROBES_DELAY: Duration = Duration::from_millis(250);
const BAUD_RATES: &[u32] = &[115_200, 57_600, 38_400];

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let serial_port = std::env::var("ZIGBEE_SERIAL_PORT")
        .map_err(|_| "ZIGBEE_SERIAL_PORT is required".to_string())?;
    let requested_mode = std::env::var("ZIGBEE_RAW_PROBE_MODE").ok();

    println!("Raw probing serial adapter on {serial_port}");

    for &baud_rate in BAUD_RATES {
        for (flow_control, label) in [
            (FlowControl::Hardware, "hardware"),
            (FlowControl::Software, "software"),
            (FlowControl::None, "none"),
        ] {
            if let Some(mode) = &requested_mode {
                if mode != label {
                    continue;
                }
            }
            println!("- opening {serial_port} at {baud_rate} baud ({label} flow control)");
            match probe_once(&serial_port, baud_rate, flow_control).await {
                Ok(()) => {}
                Err(error) => println!("  probe failed: {error}"),
            }
            sleep(BETWEEN_PROBES_DELAY).await;
        }
    }

    if let Some(mode) = requested_mode {
        println!("Completed raw probe for requested mode: {mode}");
    }

    Ok(())
}

async fn probe_once(
    serial_port: &str,
    baud_rate: u32,
    flow_control: FlowControl,
) -> Result<(), String> {
    let mut port = tokio_serial::new(serial_port, baud_rate)
        .data_bits(DataBits::Eight)
        .stop_bits(StopBits::One)
        .parity(Parity::None)
        .flow_control(flow_control)
        .open_native_async()
        .map_err(|error| format!("open failed: {error}"))?;

    let probes: &[(&str, &[u8])] = &[
        ("ash-cancel", &[0x1a]),
        ("ash-rst", &[0xc0, 0x38, 0xbc, 0x7e]),
        ("newline", b"\n"),
        ("bootloader-help", b"help\n"),
    ];

    for (label, bytes) in probes {
        println!("  sending {label}: {}", hex(bytes));
        timeout(WRITE_TIMEOUT, port.write_all(bytes))
            .await
            .map_err(|_| format!("write timeout for {label} after {:?}", WRITE_TIMEOUT))?
            .map_err(|error| format!("write failed for {label}: {error}"))?;
        timeout(WRITE_TIMEOUT, port.flush())
            .await
            .map_err(|_| format!("flush timeout for {label} after {:?}", WRITE_TIMEOUT))?
            .map_err(|error| format!("flush failed for {label}: {error}"))?;

        let mut buffer = vec![0_u8; 512];
        match timeout(READ_TIMEOUT, port.read(&mut buffer)).await {
            Ok(Ok(read)) if read > 0 => {
                buffer.truncate(read);
                println!("  recv {read} bytes: {}", hex(&buffer));
                if let Ok(text) = std::str::from_utf8(&buffer) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        println!("  text: {trimmed}");
                    }
                }
            }
            Ok(Ok(_)) => println!("  recv 0 bytes"),
            Ok(Err(error)) => println!("  read error: {error}"),
            Err(_) => println!("  recv timeout after {:?}", READ_TIMEOUT),
        }
    }

    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
