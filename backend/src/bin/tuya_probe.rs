use std::{env, net::IpAddr, str::FromStr, time::Duration};

use rust_async_tuyapi::{DpId, Payload, PayloadStruct, tuyadevice::TuyaDevice};
use serde_json::json;

#[tokio::main]
async fn main() {
    let version = env::var("TUYA_VERSION").expect("TUYA_VERSION is required");
    let id = env::var("TUYA_ID").expect("TUYA_ID is required");
    let key = env::var("TUYA_KEY").expect("TUYA_KEY is required");
    let ip = env::var("TUYA_IP").expect("TUYA_IP is required");

    let ip = IpAddr::from_str(&ip).expect("invalid TUYA_IP");
    let mut device = TuyaDevice::new(&version, &id, Some(&key), ip).expect("create TuyaDevice");
    let mut rx = device.connect().await.expect("connect");

    let query = Payload::Struct(PayloadStruct {
        gw_id: Some(id.clone()),
        dev_id: id.clone(),
        uid: Some(id.clone()),
        t: None,
        dp_id: None,
        dps: Some(json!({})),
    });

    device.get(query).await.expect("get");
    if let Ok(Some(result)) = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
        println!("GET recv: {result:#?}");
    } else {
        println!("GET recv: timeout");
    }

    let refresh = Payload::new(
        id.clone(),
        Some(id.clone()),
        Some(id.clone()),
        None,
        Some(DpId::Higher),
        None,
    );
    let _ = device.refresh(refresh).await;
    if let Ok(Some(result)) = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
        println!("REFRESH recv: {result:#?}");
    } else {
        println!("REFRESH recv: timeout");
    }

    let _ = device.disconnect().await;
}
