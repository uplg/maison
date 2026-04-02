use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

// Ear constants
pub const EAR_LEFT: u8 = 0;
pub const EAR_RIGHT: u8 = 1;
pub const EAR_DIR_FORWARD: u8 = 0;
pub const EAR_DIR_BACKWARD: u8 = 1;
pub const EAR_MAX_POSITION: u8 = 16;

// LED indices on the Nabaztag/tag (5 physical LEDs)
pub const LED_NOSE: u8 = 0;
pub const LED_LEFT: u8 = 1;
pub const LED_CENTER: u8 = 2;
pub const LED_RIGHT: u8 = 3;
pub const LED_BOTTOM: u8 = 4;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Persistent configuration stored in nabaztag.json
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NabaztagConfig {
    /// IP or hostname of the rabbit on the LAN (e.g. "192.168.1.42")
    pub host: String,
    /// Optional human-friendly name
    #[serde(default = "default_name")]
    pub name: String,
    /// Whether to enable the Tempo→LED integration
    #[serde(default)]
    pub tempo_enabled: bool,
}

fn default_name() -> String {
    "Nabaztag".to_string()
}

impl Default for NabaztagConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            name: default_name(),
            tempo_enabled: false,
        }
    }
}

/// Status returned by the Nabaztag's GET /status endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NabaztagStatus {
    #[serde(flatten)]
    pub raw: serde_json::Value,
}

/// Ear position info
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EarPosition {
    pub left: u8,
    pub right: u8,
}

/// LED color request for one or more LEDs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedColors {
    /// Nose LED color (#RRGGBB or -1 to clear)
    pub nose: Option<String>,
    /// Left body LED
    pub left: Option<String>,
    /// Center body LED
    pub center: Option<String>,
    /// Right body LED
    pub right: Option<String>,
    /// Base LED
    pub base: Option<String>,
    /// Base breathing effect (true = on, false = off)
    pub breathing: Option<bool>,
}

/// Request to move an ear
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EarMoveRequest {
    /// 0 = left, 1 = right
    pub ear: u8,
    /// Position 0..16
    pub position: u8,
    /// 0 = forward, 1 = backward
    #[serde(default)]
    pub direction: u8,
}

/// Request to play a URL
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayUrlRequest {
    pub url: String,
}

/// Request for text-to-speech
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SayRequest {
    pub text: String,
}

/// Execute arbitrary Forth code
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForthRequest {
    pub code: String,
}

/// Forth execution result
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForthResult {
    pub output: String,
}

/// Setup request (mirrors Nabaztag /setup parameters)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupRequest {
    pub latitude: Option<String>,
    pub longitude: Option<String>,
    pub language: Option<String>,
    pub taichi: Option<u16>,
    pub city_code: Option<String>,
    pub dst: Option<u8>,
    pub wake_up: Option<u8>,
    pub go_to_bed: Option<u8>,
}

/// Info service request (set a visual animation channel)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InfoServiceRequest {
    /// Service name: weather, pollution, traffic, stock, mail, service4, service5, nose
    pub service: String,
    /// Value (service-specific, typically 0..10, -1 to clear)
    pub value: i8,
}

/// Tempo color mapping for LEDs
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoLedMapping {
    pub color: String,
    pub nose: String,
    pub left: String,
    pub center: String,
    pub right: String,
    pub base: String,
}

/// Result of a Tempo push to the Nabaztag
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoPushResult {
    pub today_color: String,
    pub tomorrow_color: Option<String>,
    pub today_leds: TempoLedMapping,
    pub tomorrow_leds: Option<TempoLedMapping>,
    pub ear_left_position: u8,
    pub ear_right_position: u8,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NabaztagManager {
    config_path: Arc<PathBuf>,
    config: Arc<RwLock<NabaztagConfig>>,
    client: Client,
}

impl NabaztagManager {
    pub fn new(config_path: &Path, host_override: Option<&str>) -> Result<Self, AppError> {
        let config = if config_path.exists() {
            let content = std::fs::read_to_string(config_path)?;
            serde_json::from_str::<NabaztagConfig>(&content)?
        } else {
            NabaztagConfig::default()
        };

        // Environment variable override
        let config = if let Some(host) = host_override {
            if !host.is_empty() {
                NabaztagConfig {
                    host: host.to_string(),
                    ..config
                }
            } else {
                config
            }
        } else {
            config
        };

        let client = Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|e| AppError::http(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        Ok(Self {
            config_path: Arc::new(config_path.to_path_buf()),
            config: Arc::new(RwLock::new(config)),
            client,
        })
    }

    // -- helpers --

    fn base_url(host: &str) -> String {
        if host.starts_with("http://") || host.starts_with("https://") {
            host.trim_end_matches('/').to_string()
        } else {
            format!("http://{}", host.trim_end_matches('/'))
        }
    }

    async fn ensure_configured(&self) -> Result<String, AppError> {
        let cfg = self.config.read().await;
        if cfg.host.is_empty() {
            return Err(AppError::service_unavailable(
                "Nabaztag host not configured. Set NABAZTAG_HOST or POST /api/nabaztag/config",
            ));
        }
        Ok(Self::base_url(&cfg.host))
    }

    async fn get(&self, path: &str) -> Result<String, AppError> {
        let base = self.ensure_configured().await?;
        let url = format!("{}{}", base, path);
        debug!(url = %url, "Nabaztag GET");
        let resp = self.client.get(&url).send().await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            warn!(status = %status, body = %body, "Nabaztag returned error");
            return Err(AppError::http(
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Nabaztag returned HTTP {}: {}", status, body),
            ));
        }
        Ok(body)
    }

    async fn post_form(&self, path: &str, params: &[(&str, &str)]) -> Result<String, AppError> {
        let base = self.ensure_configured().await?;
        let url = format!("{}{}", base, path);
        debug!(url = %url, "Nabaztag POST");
        let resp = self.client.post(&url).form(params).send().await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            warn!(status = %status, body = %body, "Nabaztag POST returned error");
            return Err(AppError::http(
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Nabaztag returned HTTP {}: {}", status, body),
            ));
        }
        Ok(body)
    }

    async fn save_config(&self) -> Result<(), AppError> {
        let cfg = self.config.read().await;
        let content = serde_json::to_string_pretty(&*cfg)?;
        tokio::fs::write(self.config_path.as_ref(), content).await?;
        Ok(())
    }

    // -- public API --

    /// Get the current configuration
    pub async fn get_config(&self) -> NabaztagConfig {
        self.config.read().await.clone()
    }

    /// Update configuration
    pub async fn update_config(&self, new_config: NabaztagConfig) -> Result<NabaztagConfig, AppError> {
        {
            let mut cfg = self.config.write().await;
            *cfg = new_config;
        }
        self.save_config().await?;
        Ok(self.config.read().await.clone())
    }

    /// Get the full status from the rabbit
    pub async fn status(&self) -> Result<NabaztagStatus, AppError> {
        let body = self.get("/status").await?;
        let raw: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            AppError::http(
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Invalid JSON from Nabaztag /status: {}", e),
            )
        })?;
        Ok(NabaztagStatus { raw })
    }

    // -- Sleep / Wake --

    pub async fn sleep(&self) -> Result<(), AppError> {
        self.get("/sleep").await?;
        Ok(())
    }

    pub async fn wakeup(&self) -> Result<(), AppError> {
        self.get("/wakeup").await?;
        Ok(())
    }

    // -- Ears --

    pub async fn move_ear(&self, ear: u8, position: u8, direction: u8) -> Result<(), AppError> {
        if position > EAR_MAX_POSITION {
            return Err(AppError::http(
                axum::http::StatusCode::BAD_REQUEST,
                format!("Ear position must be 0..{}", EAR_MAX_POSITION),
            ));
        }
        let path_segment = if ear == EAR_LEFT { "left" } else { "right" };
        let url = format!("/{}?p={}&d={}", path_segment, position, direction);
        self.get(&url).await?;
        Ok(())
    }

    pub async fn get_ears(&self) -> Result<EarPosition, AppError> {
        let status = self.status().await?;
        // Parse ears from the status JSON
        let left = status.raw.pointer("/ears/left")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8;
        let right = status.raw.pointer("/ears/right")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u8;
        Ok(EarPosition { left, right })
    }

    // -- LEDs --

    pub async fn set_leds(&self, colors: &LedColors) -> Result<(), AppError> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(ref n) = colors.nose {
            params.push(("n", n.clone()));
        }
        if let Some(ref l) = colors.left {
            params.push(("l", l.clone()));
        }
        if let Some(ref c) = colors.center {
            params.push(("c", c.clone()));
        }
        if let Some(ref r) = colors.right {
            params.push(("r", r.clone()));
        }
        if let Some(ref b) = colors.base {
            params.push(("b", b.clone()));
        }
        if let Some(breathing) = colors.breathing {
            params.push(("t", if breathing { "1".to_string() } else { "0".to_string() }));
        }

        if params.is_empty() {
            return Ok(());
        }

        // Build query string for /leds
        let query: String = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        let base = self.ensure_configured().await?;
        let url = format!("{}/leds?{}", base, query);
        debug!(url = %url, "Nabaztag SET LEDs");
        let resp = self.client.post(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::http(
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Nabaztag /leds returned HTTP {}: {}", status, body),
            ));
        }
        Ok(())
    }

    pub async fn clear_leds(&self) -> Result<(), AppError> {
        self.set_leds(&LedColors {
            nose: Some("-1".to_string()),
            left: Some("-1".to_string()),
            center: Some("-1".to_string()),
            right: Some("-1".to_string()),
            base: Some("-1".to_string()),
            breathing: None,
        }).await
    }

    // -- Sound --

    pub async fn play_url(&self, url: &str) -> Result<(), AppError> {
        let encoded = urlencoding_simple(url);
        self.get(&format!("/play?u={}", encoded)).await?;
        Ok(())
    }

    pub async fn say(&self, text: &str) -> Result<(), AppError> {
        let encoded = urlencoding_simple(text);
        self.get(&format!("/say?t={}", encoded)).await?;
        Ok(())
    }

    pub async fn play_midi_communication(&self) -> Result<(), AppError> {
        self.get("/communication").await?;
        Ok(())
    }

    pub async fn play_midi_ack(&self) -> Result<(), AppError> {
        self.get("/ack").await?;
        Ok(())
    }

    pub async fn play_midi_abort(&self) -> Result<(), AppError> {
        self.get("/abort").await?;
        Ok(())
    }

    pub async fn play_midi_ministop(&self) -> Result<(), AppError> {
        self.get("/ministop").await?;
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), AppError> {
        self.get("/stop").await?;
        Ok(())
    }

    // -- Info services (LED animation channels) --

    pub async fn set_info_service(&self, service: &str, value: i8) -> Result<(), AppError> {
        let valid_services = [
            "weather", "pollution", "traffic", "stock", "mail",
            "service4", "service5", "nose",
        ];
        if !valid_services.contains(&service) {
            return Err(AppError::http(
                axum::http::StatusCode::BAD_REQUEST,
                format!(
                    "Unknown info service '{}'. Valid: {}",
                    service,
                    valid_services.join(", ")
                ),
            ));
        }
        self.get(&format!("/{}?v={}", service, value)).await?;
        Ok(())
    }

    pub async fn clear_info(&self) -> Result<(), AppError> {
        self.get("/clear").await?;
        Ok(())
    }

    // -- Utility --

    pub async fn taichi(&self) -> Result<(), AppError> {
        self.get("/taichi").await?;
        Ok(())
    }

    pub async fn surprise(&self) -> Result<(), AppError> {
        self.get("/surprise").await?;
        Ok(())
    }

    pub async fn reboot(&self) -> Result<(), AppError> {
        self.get("/reboot").await?;
        Ok(())
    }

    pub async fn update_time(&self) -> Result<(), AppError> {
        self.get("/update-time").await?;
        Ok(())
    }

    pub async fn get_animations(&self) -> Result<serde_json::Value, AppError> {
        let body = self.get("/animations").await?;
        let val: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
        Ok(val)
    }

    pub async fn get_tasks(&self) -> Result<serde_json::Value, AppError> {
        let body = self.get("/tasks").await?;
        let val: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
        Ok(val)
    }

    // -- Setup --

    pub async fn setup(&self, req: &SetupRequest) -> Result<(), AppError> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(ref lat) = req.latitude {
            params.push(("j", lat.clone()));
        }
        if let Some(ref lon) = req.longitude {
            params.push(("k", lon.clone()));
        }
        if let Some(ref lang) = req.language {
            params.push(("l", lang.clone()));
        }
        if let Some(taichi) = req.taichi {
            params.push(("t", taichi.to_string()));
        }
        if let Some(ref city) = req.city_code {
            params.push(("c", city.clone()));
        }
        if let Some(dst) = req.dst {
            params.push(("d", dst.to_string()));
        }
        if let Some(wake) = req.wake_up {
            params.push(("w", wake.to_string()));
        }
        if let Some(bed) = req.go_to_bed {
            params.push(("b", bed.to_string()));
        }

        let query: String = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        if !query.is_empty() {
            self.get(&format!("/setup?{}", query)).await?;
        }
        Ok(())
    }

    // -- Forth interpreter --

    pub async fn execute_forth(&self, code: &str) -> Result<ForthResult, AppError> {
        let output = self.post_form("/forth", &[("c", code)]).await?;
        Ok(ForthResult { output })
    }

    // -- Tempo integration --

    /// Map a Tempo color name to LED colors for the Nabaztag
    fn tempo_color_to_leds(color: &str) -> TempoLedMapping {
        match color.to_uppercase().as_str() {
            "BLUE" | "BLEU" => TempoLedMapping {
                color: color.to_string(),
                nose: "#0000ff".to_string(),
                left: "#0000ff".to_string(),
                center: "#0044ff".to_string(),
                right: "#0000ff".to_string(),
                base: "#000088".to_string(),
            },
            "WHITE" | "BLANC" => TempoLedMapping {
                color: color.to_string(),
                nose: "#ffffff".to_string(),
                left: "#ffffff".to_string(),
                center: "#ffffff".to_string(),
                right: "#ffffff".to_string(),
                base: "#888888".to_string(),
            },
            "RED" | "ROUGE" => TempoLedMapping {
                color: color.to_string(),
                nose: "#ff0000".to_string(),
                left: "#ff0000".to_string(),
                center: "#ff2200".to_string(),
                right: "#ff0000".to_string(),
                base: "#880000".to_string(),
            },
            _ => TempoLedMapping {
                color: color.to_string(),
                nose: "#444444".to_string(),
                left: "#444444".to_string(),
                center: "#444444".to_string(),
                right: "#444444".to_string(),
                base: "#222222".to_string(),
            },
        }
    }

    /// Map a Tempo color to an ear position (symbolic: blue=low, white=mid, red=high)
    fn tempo_color_to_ear_position(color: &str) -> u8 {
        match color.to_uppercase().as_str() {
            "BLUE" | "BLEU" => 0,      // ears down = cheap, relax
            "WHITE" | "BLANC" => 8,     // ears mid = moderate
            "RED" | "ROUGE" => 16,      // ears fully up = expensive, alert!
            _ => 4,
        }
    }

    /// Push Tempo day colors to the Nabaztag LEDs and ears.
    ///
    /// Strategy:
    /// - Today's color → left LED + center LED + nose
    /// - Tomorrow's color (if known) → right LED + base LED
    /// - Ears: left ear = today severity, right ear = tomorrow severity
    pub async fn push_tempo(
        &self,
        today_color: &str,
        tomorrow_color: Option<&str>,
    ) -> Result<TempoPushResult, AppError> {
        let today = Self::tempo_color_to_leds(today_color);
        let tomorrow = tomorrow_color.map(Self::tempo_color_to_leds);

        let left_ear_pos = Self::tempo_color_to_ear_position(today_color);
        let right_ear_pos = tomorrow_color
            .map(Self::tempo_color_to_ear_position)
            .unwrap_or(left_ear_pos);

        // Set LEDs: today on left side, tomorrow on right side
        let led_colors = LedColors {
            nose: Some(today.nose.clone()),
            left: Some(today.left.clone()),
            center: Some(today.center.clone()),
            right: tomorrow.as_ref().map(|t| t.right.clone()).or_else(|| Some(today.right.clone())),
            base: tomorrow.as_ref().map(|t| t.base.clone()).or_else(|| Some(today.base.clone())),
            breathing: Some(today_color.eq_ignore_ascii_case("RED") || today_color.eq_ignore_ascii_case("ROUGE")),
        };
        self.set_leds(&led_colors).await?;

        // Set ears
        let _ = self.move_ear(EAR_LEFT, left_ear_pos, EAR_DIR_FORWARD).await;
        let _ = self.move_ear(EAR_RIGHT, right_ear_pos, EAR_DIR_FORWARD).await;

        Ok(TempoPushResult {
            today_color: today_color.to_string(),
            tomorrow_color: tomorrow_color.map(|s| s.to_string()),
            today_leds: today,
            tomorrow_leds: tomorrow,
            ear_left_position: left_ear_pos,
            ear_right_position: right_ear_pos,
        })
    }

    /// Check if Tempo integration is enabled
    pub async fn is_tempo_enabled(&self) -> bool {
        self.config.read().await.tempo_enabled
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Minimal URL encoding (the Nabaztag firmware is basic, so we keep it simple)
fn urlencoding_simple(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '#' => "%23".to_string(),
            '&' => "%26".to_string(),
            '?' => "%3F".to_string(),
            '=' => "%3D".to_string(),
            '+' => "%2B".to_string(),
            '%' => "%25".to_string(),
            _ if c.is_ascii_alphanumeric() || "-._~:/!$'()*,;@".contains(c) => {
                c.to_string()
            }
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}
