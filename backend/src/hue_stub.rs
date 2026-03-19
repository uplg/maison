use serde::Serialize;

use crate::{config::Config, error::AppError};

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

#[derive(Clone, Default)]
pub struct HueManager;

impl HueManager {
    pub fn new(_config: &Config) -> Result<Self, AppError> {
        Ok(Self)
    }

    pub async fn shutdown(&self) {}

    pub async fn list_lamps(&self) -> Vec<HueLampView> {
        Vec::new()
    }

    pub async fn get_lamp(&self, _lamp_id: &str) -> Option<HueLampView> {
        None
    }

    pub async fn stats(&self) -> HueStats {
        HueStats {
            total: 0,
            connected: 0,
            reachable: 0,
            disabled: true,
            message: Some("Hue Bluetooth support is not built into this binary".to_string()),
        }
    }

    pub async fn trigger_scan(&self) -> Result<(), AppError> {
        Err(bluetooth_unavailable())
    }

    pub async fn connect_all(&self) {}

    pub async fn disconnect_all(&self) {}

    pub async fn connect_lamp(&self, _lamp_id: &str) -> Result<bool, AppError> {
        Ok(false)
    }

    pub async fn disconnect_lamp(&self, _lamp_id: &str) -> Result<(), AppError> {
        Ok(())
    }

    pub async fn refresh_lamp_state(&self, _lamp_id: &str) -> Result<Option<HueLampState>, AppError> {
        Ok(None)
    }

    pub async fn set_power(&self, _lamp_id: &str, _enabled: bool) -> Result<HueLampState, AppError> {
        Err(bluetooth_unavailable())
    }

    pub async fn set_brightness(&self, _lamp_id: &str, _brightness: u8) -> Result<HueLampState, AppError> {
        Err(bluetooth_unavailable())
    }

    pub async fn set_temperature(&self, _lamp_id: &str, _temperature: u8) -> Result<HueLampState, AppError> {
        Err(bluetooth_unavailable())
    }

    pub async fn set_lamp_state(
        &self,
        _lamp_id: &str,
        _is_on: bool,
        _brightness: Option<u8>,
    ) -> Result<HueLampState, AppError> {
        Err(bluetooth_unavailable())
    }

    pub async fn rename_lamp(&self, _lamp_id: &str, _name: &str) -> Result<bool, AppError> {
        Err(bluetooth_unavailable())
    }

    pub async fn blacklist_lamp(&self, _lamp_id: &str) -> Result<bool, AppError> {
        Err(bluetooth_unavailable())
    }
}

fn bluetooth_unavailable() -> AppError {
    AppError::service_unavailable("Hue Bluetooth support is not built into this binary")
}
