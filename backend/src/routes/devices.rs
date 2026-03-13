use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde::Serialize;
use serde_json::{Value, json};

use crate::{AppState, auth::AuthenticatedUser, error::AppError, tuya};

const FEEDER_MAX_PORTIONS: u64 = 10;
const FEEDER_WARN_PORTIONS: u64 = 12;
const LITTER_MAX_CLEAN_DELAY_SECONDS: u64 = 1800;
const FOUNTAIN_MAX_UV_RUNTIME_HOURS: u64 = 24;

#[derive(Debug, Serialize)]
struct DevicesListResponse {
    success: bool,
    devices: Vec<tuya::TuyaDeviceListEntry>,
    total: usize,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct ConnectionResponse {
    success: bool,
    message: String,
}

#[derive(Debug, Serialize)]
struct StatsResponse {
    success: bool,
    total: usize,
    connected: usize,
    disconnected: usize,
    devices: Vec<DeviceConnectionStatsEntry>,
}

#[derive(Debug, Serialize)]
struct DeviceConnectionStatsEntry {
    id: String,
    name: String,
    #[serde(rename = "type")]
    device_type: String,
    #[serde(rename = "isConnected")]
    is_connected: bool,
    connecting: bool,
    #[serde(rename = "reconnectAttempts")]
    reconnect_attempts: i32,
}

#[derive(Debug, Serialize)]
struct DpsScanResponse {
    success: bool,
    scan_range: String,
    scanned_count: usize,
    found_count: usize,
    available_dps: std::collections::BTreeMap<String, DpsValueSummary>,
    errors_count: usize,
    errors: Option<std::collections::BTreeMap<String, String>>,
    message: String,
}

#[derive(Debug, Serialize)]
struct DpsValueSummary {
    value: Value,
    #[serde(rename = "type")]
    value_type: String,
    length: Option<usize>,
}

#[derive(Debug, Serialize)]
struct DeviceStatusResponse {
    success: bool,
    device: tuya::TuyaDeviceRef,
    parsed_status: serde_json::Value,
    raw_status: serde_json::Map<String, serde_json::Value>,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct TypedStatusResponse {
    success: bool,
    device: tuya::TuyaDeviceRef,
    parsed_status: serde_json::Value,
    raw_dps: serde_json::Map<String, serde_json::Value>,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct ActionResponse {
    success: bool,
    message: String,
    device: tuya::TuyaDeviceRef,
}

#[derive(Debug, Serialize)]
struct MealPlanResponse {
    success: bool,
    device: tuya::TuyaDeviceRef,
    decoded: Option<Vec<MealPlanEntry>>,
    meal_plan: Option<String>,
    message: String,
}

#[derive(Debug, Serialize)]
struct MealPlanUpdateResponse {
    success: bool,
    message: String,
    device: tuya::TuyaDeviceRef,
    encoded_base64: String,
    formatted_meal_plan: String,
}

#[derive(Debug, Serialize)]
struct LitterBoxSettingsResponse {
    success: bool,
    message: String,
    device: tuya::TuyaDeviceRef,
    updated_settings: usize,
}

#[derive(Debug, Serialize)]
struct FountainUvResponse {
    success: bool,
    message: String,
    device: tuya::TuyaDeviceRef,
    applied_settings: FountainUvAppliedSettings,
}

#[derive(Debug, Serialize)]
struct FountainUvAppliedSettings {
    enabled: Option<bool>,
    runtime: Option<u64>,
}

#[derive(Debug, Serialize)]
struct FountainEcoModeResponse {
    success: bool,
    message: String,
    device: tuya::TuyaDeviceRef,
    eco_mode: u64,
}

#[derive(Debug, Serialize)]
struct FountainPowerResponse {
    success: bool,
    message: String,
    device: tuya::TuyaDeviceRef,
    power: bool,
}

#[derive(Debug, Deserialize)]
struct FeedRequest {
    #[serde(default = "default_feeder_portion")]
    portion: u64,
}

#[derive(Debug, Deserialize)]
struct MealPlanRequest {
    meal_plan: Vec<MealPlanEntry>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct MealPlanEntry {
    days_of_week: Vec<String>,
    time: String,
    portion: u8,
    status: String,
}

#[derive(Debug, Deserialize)]
struct LitterBoxSettingsRequest {
    clean_delay: Option<u64>,
    sleep_mode: Option<LitterSleepModeSettings>,
    preferences: Option<LitterPreferences>,
    actions: Option<LitterActions>,
}

#[derive(Debug, Deserialize)]
struct LitterSleepModeSettings {
    enabled: Option<bool>,
    start_time: Option<String>,
    end_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LitterPreferences {
    child_lock: Option<bool>,
    kitten_mode: Option<bool>,
    lighting: Option<bool>,
    prompt_sound: Option<bool>,
    automatic_homing: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct LitterActions {
    reset_sand_level: Option<bool>,
    reset_factory_settings: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct FountainUvSettingsRequest {
    enabled: Option<bool>,
    runtime: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct FountainEcoModeRequest {
    mode: u64,
}

#[derive(Debug, Deserialize)]
struct FountainPowerRequest {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct ScanDpsQuery {
    start: Option<String>,
    end: Option<String>,
    timeout: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_devices))
        .route("/stats", get(stats))
        .route("/reconnect", post(reconnect))
        .route("/connect", post(connect_all))
        .route("/disconnect", post(disconnect_all))
        .route("/{device_id}/connect", get(connect_device))
        .route("/{device_id}/disconnect", get(disconnect_device))
        .route("/{device_id}/status", get(status))
        .route("/{device_id}/scan-dps", get(scan_dps))
        .route("/{device_id}/feeder/feed", post(feeder_feed))
        .route("/{device_id}/feeder/status", get(feeder_status))
        .route("/{device_id}/feeder/meal-plan", get(feeder_meal_plan).post(update_feeder_meal_plan))
        .route("/{device_id}/litter-box/clean", post(litter_box_clean))
        .route("/{device_id}/litter-box/settings", post(update_litter_box_settings))
        .route("/{device_id}/litter-box/status", get(litter_box_status))
        .route("/{device_id}/fountain/reset/water", post(fountain_reset_water))
        .route("/{device_id}/fountain/reset/filter", post(fountain_reset_filter))
        .route("/{device_id}/fountain/reset/pump", post(fountain_reset_pump))
        .route("/{device_id}/fountain/uv", post(update_fountain_uv))
        .route("/{device_id}/fountain/eco-mode", post(update_fountain_eco_mode))
        .route("/{device_id}/fountain/power", post(update_fountain_power))
        .route("/{device_id}/fountain/status", get(fountain_status))
}

async fn list_devices(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<DevicesListResponse>, AppError> {
    let _ = user.0;
    let devices = state.tuya.list_devices().await;
    Ok(Json(DevicesListResponse {
        success: true,
        total: devices.len(),
        devices,
        message: "Devices list retrieved successfully",
    }))
}

async fn status(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<DeviceStatusResponse>, AppError> {
    let _ = user.0;
    let (device, raw_status, parsed_status) = state.tuya.get_status(&device_id).await?;
    Ok(Json(DeviceStatusResponse {
        success: true,
        device,
        parsed_status,
        raw_status,
        message: "Device status retrieved successfully",
    }))
}

async fn stats(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<StatsResponse>, AppError> {
    let _ = user.0;
    let stats = state.tuya.connection_stats().await;

    Ok(Json(StatsResponse {
        success: true,
        total: stats.total,
        connected: stats.connected,
        disconnected: stats.disconnected,
        devices: stats
            .devices
            .into_iter()
            .map(|device| DeviceConnectionStatsEntry {
                id: device.id,
                name: device.name,
                device_type: device.device_type,
                is_connected: device.connected,
                connecting: device.connecting,
                reconnect_attempts: device.reconnect_attempts,
            })
            .collect(),
    }))
}

async fn reconnect(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<ConnectionResponse>, AppError> {
    let _ = user.0;
    state.tuya.reconnect_disconnected().await;
    Ok(Json(ConnectionResponse {
        success: true,
        message: "Reconnection initiated for disconnected devices".to_string(),
    }))
}

async fn connect_all(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<ConnectionResponse>, AppError> {
    let _ = user.0;
    let device_ids = state.tuya.list_devices().await.into_iter().map(|device| device.id).collect::<Vec<_>>();
    for device_id in device_ids {
        let _ = state.tuya.connect_device(&device_id).await;
    }
    Ok(Json(ConnectionResponse {
        success: true,
        message: "All devices connection initiated".to_string(),
    }))
}

async fn disconnect_all(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<ConnectionResponse>, AppError> {
    let _ = user.0;
    state.tuya.disconnect_all_devices().await;
    Ok(Json(ConnectionResponse {
        success: true,
        message: "All devices disconnected".to_string(),
    }))
}

async fn connect_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<ConnectionResponse>, AppError> {
    let _ = user.0;
    let device = state.tuya.get_device_ref(&device_id)?;
    let _ = state.tuya.connect_device(&device_id).await?;
    Ok(Json(ConnectionResponse {
        success: true,
        message: format!("Device {} connection initiated", device.id),
    }))
}

async fn disconnect_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<ConnectionResponse>, AppError> {
    let _ = user.0;
    let device = state.tuya.get_device_ref(&device_id)?;
    state.tuya.disconnect_device(&device_id).await?;
    Ok(Json(ConnectionResponse {
        success: true,
        message: format!("Device {} disconnected", device.id),
    }))
}

async fn scan_dps(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<ScanDpsQuery>,
    user: AuthenticatedUser,
) -> Result<Json<DpsScanResponse>, AppError> {
    let _ = user.0;
    let _device = state.tuya.get_device_ref(&device_id)?;

    let start = query
        .start
        .as_deref()
        .unwrap_or("1")
        .parse::<u32>()
        .map_err(|_| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid start DPS"))?;
    let end = query
        .end
        .as_deref()
        .unwrap_or("255")
        .parse::<u32>()
        .map_err(|_| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid end DPS"))?;
    let _timeout = query
        .timeout
        .as_deref()
        .unwrap_or("3000")
        .parse::<u32>()
        .map_err(|_| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid timeout"))?;

    if start == 0 || end < start {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid DPS scan range",
        ));
    }

    let (_, raw_status, _) = state.tuya.get_status(&device_id).await?;
    let mut available_dps = std::collections::BTreeMap::new();

    for dps in start..=end {
        let key = dps.to_string();
        if let Some(value) = raw_status.get(&key) {
            available_dps.insert(
                key,
                DpsValueSummary {
                    value: value.clone(),
                    value_type: dps_value_type(value),
                    length: value.as_str().map(str::len),
                },
            );
        }
    }

    let scanned_count = usize::try_from(end - start + 1)
        .map_err(|_| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid DPS scan range"))?;
    let found_count = available_dps.len();

    Ok(Json(DpsScanResponse {
        success: true,
        scan_range: format!("{start}-{end}"),
        scanned_count,
        found_count,
        available_dps,
        errors_count: 0,
        errors: None,
        message: format!(
            "DPS scan completed: {found_count} active DPS found out of {scanned_count} scanned"
        ),
    }))
}

async fn feeder_status(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<TypedStatusResponse>, AppError> {
    let _ = user.0;
    let (device, raw_dps, parsed_status) = state
        .tuya
        .get_typed_status(&device_id, tuya::TuyaDeviceType::Feeder)
        .await?;
    Ok(Json(TypedStatusResponse {
        success: true,
        device,
        parsed_status,
        raw_dps,
        message: "Feeder status retrieved successfully",
    }))
}

async fn feeder_feed(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<FeedRequest>,
) -> Result<Json<ActionResponse>, AppError> {
    let _ = user.0;
    if !(1..=FEEDER_WARN_PORTIONS).contains(&body.portion) {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            format!("portion must be between 1 and {FEEDER_WARN_PORTIONS}"),
        ));
    }

    let device = state
        .tuya
        .send_typed_command(&device_id, tuya::TuyaDeviceType::Feeder, "3", json!(body.portion))
        .await?;

    Ok(Json(ActionResponse {
        success: true,
        message: format!(
            "Manual feed command sent to {} with portions: {}",
            device.name, body.portion
        ),
        device,
    }))
}

async fn feeder_meal_plan(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<MealPlanResponse>, AppError> {
    let _ = user.0;
    let (device, meal_plan) = state.tuya.feeder_meal_plan(&device_id).await?;
    let decoded = meal_plan
        .as_deref()
        .map(decode_meal_plan)
        .transpose()?;

    let message = if meal_plan.is_some() {
        "Current meal plan retrieved"
    } else {
        "Meal plan not available yet."
    };

    Ok(Json(MealPlanResponse {
        success: true,
        device,
        decoded,
        meal_plan,
        message: message.to_string(),
    }))
}

async fn update_feeder_meal_plan(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<MealPlanRequest>,
) -> Result<Json<MealPlanUpdateResponse>, AppError> {
    let _ = user.0;

    if body.meal_plan.is_empty() {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "meal_plan array is required",
        ));
    }

    if body.meal_plan.len() > 10 {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "meal_plan supports at most 10 entries",
        ));
    }

    for (index, entry) in body.meal_plan.iter().enumerate() {
        validate_meal_plan_entry(entry, index)?;
    }

    let encoded = encode_meal_plan(&body.meal_plan)?;
    let device = state
        .tuya
        .send_typed_command(&device_id, tuya::TuyaDeviceType::Feeder, "1", json!(encoded))
        .await?;

    Ok(Json(MealPlanUpdateResponse {
        success: true,
        message: format!("Meal plan updated for {}", device.name),
        device,
        encoded_base64: encoded,
        formatted_meal_plan: format_meal_plan(&body.meal_plan),
    }))
}

async fn litter_box_status(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<TypedStatusResponse>, AppError> {
    let _ = user.0;
    let (device, raw_dps, parsed_status) = state
        .tuya
        .get_typed_status(&device_id, tuya::TuyaDeviceType::LitterBox)
        .await?;
    Ok(Json(TypedStatusResponse {
        success: true,
        device,
        parsed_status,
        raw_dps,
        message: "Litter box status retrieved successfully",
    }))
}

async fn litter_box_clean(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<ActionResponse>, AppError> {
    let _ = user.0;
    let device = state
        .tuya
        .send_typed_command(&device_id, tuya::TuyaDeviceType::LitterBox, "107", Value::Bool(true))
        .await?;

    Ok(Json(ActionResponse {
        success: true,
        message: format!("Manual cleaning cycle initiated for {}", device.name),
        device,
    }))
}

async fn update_litter_box_settings(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<LitterBoxSettingsRequest>,
) -> Result<Json<LitterBoxSettingsResponse>, AppError> {
    let _ = user.0;
    let mut updates = Vec::new();

    if let Some(clean_delay) = body.clean_delay {
        if clean_delay > LITTER_MAX_CLEAN_DELAY_SECONDS {
            return Err(AppError::http(
                axum::http::StatusCode::BAD_REQUEST,
                "clean_delay must be between 0 and 1800 seconds",
            ));
        }
        updates.push(("101".to_string(), json!(clean_delay)));
    }

    if let Some(sleep_mode) = body.sleep_mode {
        if let Some(enabled) = sleep_mode.enabled {
            updates.push(("102".to_string(), Value::Bool(enabled)));
        }
        if let Some(start_time) = sleep_mode.start_time {
            updates.push(("103".to_string(), json!(parse_hhmm_to_minutes(&start_time)?)));
        }
        if let Some(end_time) = sleep_mode.end_time {
            updates.push(("104".to_string(), json!(parse_hhmm_to_minutes(&end_time)?)));
        }
    }

    if let Some(preferences) = body.preferences {
        push_optional_bool(&mut updates, "110", preferences.child_lock);
        push_optional_bool(&mut updates, "111", preferences.kitten_mode);
        push_optional_bool(&mut updates, "116", preferences.lighting);
        push_optional_bool(&mut updates, "117", preferences.prompt_sound);
        push_optional_bool(&mut updates, "119", preferences.automatic_homing);
    }

    if let Some(actions) = body.actions {
        if actions.reset_sand_level == Some(true) {
            updates.push(("113".to_string(), Value::Bool(true)));
        }
        if actions.reset_factory_settings == Some(true) {
            updates.push(("115".to_string(), Value::Bool(true)));
        }
    }

    if updates.is_empty() {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "No valid settings provided",
        ));
    }

    let updated_settings = updates.len();
    let device = state
        .tuya
        .send_typed_commands(&device_id, tuya::TuyaDeviceType::LitterBox, updates)
        .await?;

    Ok(Json(LitterBoxSettingsResponse {
        success: true,
        message: format!("Settings updated for {}", device.name),
        device,
        updated_settings,
    }))
}

async fn fountain_status(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<TypedStatusResponse>, AppError> {
    let _ = user.0;
    let (device, raw_dps, parsed_status) = state
        .tuya
        .get_typed_status(&device_id, tuya::TuyaDeviceType::Fountain)
        .await?;
    Ok(Json(TypedStatusResponse {
        success: true,
        device,
        parsed_status,
        raw_dps,
        message: "Fountain status retrieved successfully",
    }))
}

async fn fountain_reset_water(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<ActionResponse>, AppError> {
    let _ = user.0;
    let device = state
        .tuya
        .send_typed_command(&device_id, tuya::TuyaDeviceType::Fountain, "6", json!(0))
        .await?;

    Ok(Json(ActionResponse {
        success: true,
        message: format!("Water time counter reset for {}", device.name),
        device,
    }))
}

async fn fountain_reset_filter(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<ActionResponse>, AppError> {
    let _ = user.0;
    let device = state
        .tuya
        .send_typed_command(&device_id, tuya::TuyaDeviceType::Fountain, "7", Value::Bool(true))
        .await?;

    Ok(Json(ActionResponse {
        success: true,
        message: format!("Filter life counter reset for {}", device.name),
        device,
    }))
}

async fn fountain_reset_pump(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<ActionResponse>, AppError> {
    let _ = user.0;
    let device = state
        .tuya
        .send_typed_command(&device_id, tuya::TuyaDeviceType::Fountain, "8", Value::Bool(true))
        .await?;

    Ok(Json(ActionResponse {
        success: true,
        message: format!("Pump time counter reset for {}", device.name),
        device,
    }))
}

async fn update_fountain_uv(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<FountainUvSettingsRequest>,
) -> Result<Json<FountainUvResponse>, AppError> {
    let _ = user.0;
    let mut updates = Vec::new();

    if let Some(enabled) = body.enabled {
        updates.push(("10".to_string(), Value::Bool(enabled)));
    }
    if let Some(runtime) = body.runtime {
        if runtime > FOUNTAIN_MAX_UV_RUNTIME_HOURS {
            return Err(AppError::http(
                axum::http::StatusCode::BAD_REQUEST,
                "UV runtime must be between 0 and 24 hours",
            ));
        }
        updates.push(("11".to_string(), json!(runtime)));
    }

    if updates.is_empty() {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "No valid settings provided",
        ));
    }

    let applied_settings = FountainUvAppliedSettings {
        enabled: body.enabled,
        runtime: body.runtime,
    };
    let summary = describe_fountain_uv_updates(&applied_settings);
    let device = state
        .tuya
        .send_typed_commands(&device_id, tuya::TuyaDeviceType::Fountain, updates)
        .await?;

    Ok(Json(FountainUvResponse {
        success: true,
        message: format!("UV settings updated for {}: {summary}", device.name),
        device,
        applied_settings,
    }))
}

async fn update_fountain_eco_mode(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<FountainEcoModeRequest>,
) -> Result<Json<FountainEcoModeResponse>, AppError> {
    let _ = user.0;
    if !(1..=2).contains(&body.mode) {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "Eco mode must be 1 or 2",
        ));
    }

    let device = state
        .tuya
        .send_typed_command(&device_id, tuya::TuyaDeviceType::Fountain, "102", json!(body.mode))
        .await?;

    Ok(Json(FountainEcoModeResponse {
        success: true,
        message: format!("Eco mode set to {} for {}", body.mode, device.name),
        device,
        eco_mode: body.mode,
    }))
}

async fn update_fountain_power(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<FountainPowerRequest>,
) -> Result<Json<FountainPowerResponse>, AppError> {
    let _ = user.0;
    let device = state
        .tuya
        .send_typed_command(&device_id, tuya::TuyaDeviceType::Fountain, "1", Value::Bool(body.enabled))
        .await?;

    Ok(Json(FountainPowerResponse {
        success: true,
        message: format!(
            "Light {} for {}",
            if body.enabled { "turned on" } else { "turned off" },
            device.name
        ),
        device,
        power: body.enabled,
    }))
}

fn default_feeder_portion() -> u64 {
    1
}

fn push_optional_bool(updates: &mut Vec<(String, Value)>, dps: &str, value: Option<bool>) {
    if let Some(value) = value {
        updates.push((dps.to_string(), Value::Bool(value)));
    }
}

fn parse_hhmm_to_minutes(value: &str) -> Result<u64, AppError> {
    let mut parts = value.split(':');
    let hours = parts
        .next()
        .ok_or_else(|| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid time format. Use HH:MM"))?
        .parse::<u64>()
        .map_err(|_| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid time format. Use HH:MM"))?;
    let minutes = parts
        .next()
        .ok_or_else(|| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid time format. Use HH:MM"))?
        .parse::<u64>()
        .map_err(|_| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid time format. Use HH:MM"))?;

    if parts.next().is_some() || hours > 23 || minutes > 59 {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid time format. Use HH:MM",
        ));
    }

    Ok((hours * 60) + minutes)
}

fn validate_meal_plan_entry(entry: &MealPlanEntry, index: usize) -> Result<(), AppError> {
    if entry.days_of_week.is_empty() {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            format!("Invalid meal plan entry at index {index}"),
        ));
    }

    for day in &entry.days_of_week {
        if day_index(day).is_none() {
            return Err(AppError::http(
                axum::http::StatusCode::BAD_REQUEST,
                format!("Invalid meal plan entry at index {index}"),
            ));
        }
    }

    let _ = parse_hhmm_to_minutes(&entry.time).map_err(|_| {
        AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            format!("Invalid meal plan entry at index {index}"),
        )
    })?;

    if !(1..=FEEDER_MAX_PORTIONS as u8).contains(&entry.portion) {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            format!("Invalid meal plan entry at index {index}"),
        ));
    }

    if entry.status != "Enabled" && entry.status != "Disabled" {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            format!("Invalid meal plan entry at index {index}"),
        ));
    }

    Ok(())
}

fn encode_meal_plan(entries: &[MealPlanEntry]) -> Result<String, AppError> {
    let mut encoded = Vec::with_capacity(entries.len() * 5);
    for entry in entries {
        let days_bits = entry
            .days_of_week
            .iter()
            .try_fold(0_u8, |acc, day| {
                day_index(day)
                    .map(|index| acc | (1 << index))
                    .ok_or_else(|| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid day of week"))
            })?;
        let total_minutes = parse_hhmm_to_minutes(&entry.time)?;
        let hours = u8::try_from(total_minutes / 60)
            .map_err(|_| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid time format. Use HH:MM"))?;
        let minutes = u8::try_from(total_minutes % 60)
            .map_err(|_| AppError::http(axum::http::StatusCode::BAD_REQUEST, "Invalid time format. Use HH:MM"))?;
        let status = if entry.status == "Enabled" { 1 } else { 0 };

        encoded.extend_from_slice(&[days_bits, hours, minutes, entry.portion, status]);
    }

    Ok(STANDARD.encode(encoded))
}

fn decode_meal_plan(encoded: &str) -> Result<Vec<MealPlanEntry>, AppError> {
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|error| AppError::service_unavailable(error.to_string()))?;
    let mut entries = Vec::new();

    for chunk in bytes.chunks(5) {
        if chunk.len() < 5 {
            break;
        }

        let days_of_week = (0..7)
            .filter(|index| chunk[0] & (1 << index) != 0)
            .map(day_name)
            .collect::<Vec<_>>();
        let time = format!("{:02}:{:02}", chunk[1], chunk[2]);
        let status = if chunk[4] == 1 { "Enabled" } else { "Disabled" };

        entries.push(MealPlanEntry {
            days_of_week,
            time,
            portion: chunk[3],
            status: status.to_string(),
        });
    }

    Ok(entries)
}

fn format_meal_plan(entries: &[MealPlanEntry]) -> String {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            format!(
                "{}. {} a {} - {} serving(s) - {}",
                index + 1,
                entry.days_of_week.join(", "),
                entry.time,
                entry.portion,
                entry.status,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn day_index(day: &str) -> Option<u8> {
    match day {
        "Monday" => Some(0),
        "Tuesday" => Some(1),
        "Wednesday" => Some(2),
        "Thursday" => Some(3),
        "Friday" => Some(4),
        "Saturday" => Some(5),
        "Sunday" => Some(6),
        _ => None,
    }
}

fn day_name(index: u8) -> String {
    match index {
        0 => "Monday",
        1 => "Tuesday",
        2 => "Wednesday",
        3 => "Thursday",
        4 => "Friday",
        5 => "Saturday",
        6 => "Sunday",
        _ => "Unknown",
    }
    .to_string()
}

fn describe_fountain_uv_updates(settings: &FountainUvAppliedSettings) -> String {
    let mut parts = Vec::new();
    if let Some(enabled) = settings.enabled {
        parts.push(if enabled {
            "UV light enabled".to_string()
        } else {
            "UV light disabled".to_string()
        });
    }
    if let Some(runtime) = settings.runtime {
        parts.push(format!("UV runtime set to {runtime} hours"));
    }
    parts.join(", ")
}

fn dps_value_type(value: &Value) -> String {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hhmm_to_minutes_accepts_valid_times() {
        assert_eq!(parse_hhmm_to_minutes("00:00").unwrap(), 0);
        assert_eq!(parse_hhmm_to_minutes("08:30").unwrap(), 510);
        assert_eq!(parse_hhmm_to_minutes("23:59").unwrap(), 1_439);
    }

    #[test]
    fn parse_hhmm_to_minutes_rejects_invalid_times() {
        let error = parse_hhmm_to_minutes("24:00").unwrap_err();
        assert_eq!(error.to_string(), "Invalid time format. Use HH:MM");

        let error = parse_hhmm_to_minutes("bad").unwrap_err();
        assert_eq!(error.to_string(), "Invalid time format. Use HH:MM");
    }

    #[test]
    fn validate_meal_plan_entry_accepts_valid_entry() {
        let entry = MealPlanEntry {
            days_of_week: vec!["Monday".to_string(), "Wednesday".to_string()],
            time: "08:30".to_string(),
            portion: 2,
            status: "Enabled".to_string(),
        };

        validate_meal_plan_entry(&entry, 0).unwrap();
    }

    #[test]
    fn validate_meal_plan_entry_rejects_invalid_day() {
        let entry = MealPlanEntry {
            days_of_week: vec!["Funday".to_string()],
            time: "08:30".to_string(),
            portion: 2,
            status: "Enabled".to_string(),
        };

        let error = validate_meal_plan_entry(&entry, 0).unwrap_err();
        assert_eq!(error.to_string(), "Invalid meal plan entry at index 0");
    }

    #[test]
    fn validate_meal_plan_entry_rejects_invalid_portion() {
        let entry = MealPlanEntry {
            days_of_week: vec!["Monday".to_string()],
            time: "08:30".to_string(),
            portion: 11,
            status: "Enabled".to_string(),
        };

        let error = validate_meal_plan_entry(&entry, 3).unwrap_err();
        assert_eq!(error.to_string(), "Invalid meal plan entry at index 3");
    }

    #[test]
    fn encode_meal_plan_matches_legacy_format() {
        let entries = vec![MealPlanEntry {
            days_of_week: vec!["Monday".to_string(), "Wednesday".to_string()],
            time: "08:30".to_string(),
            portion: 2,
            status: "Enabled".to_string(),
        }];

        let encoded = encode_meal_plan(&entries).unwrap();
        assert_eq!(encoded, "BQgeAgE=");
    }

    #[test]
    fn decode_meal_plan_matches_legacy_payload() {
        let decoded = decode_meal_plan("BQgeAgE=").unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].days_of_week, vec!["Monday", "Wednesday"]);
        assert_eq!(decoded[0].time, "08:30");
        assert_eq!(decoded[0].portion, 2);
        assert_eq!(decoded[0].status, "Enabled");
    }

    #[test]
    fn encode_then_decode_meal_plan_roundtrips() {
        let entries = vec![
            MealPlanEntry {
                days_of_week: vec!["Monday".to_string(), "Friday".to_string()],
                time: "07:15".to_string(),
                portion: 1,
                status: "Enabled".to_string(),
            },
            MealPlanEntry {
                days_of_week: vec!["Sunday".to_string()],
                time: "18:45".to_string(),
                portion: 3,
                status: "Disabled".to_string(),
            },
        ];

        let encoded = encode_meal_plan(&entries).unwrap();
        let decoded = decode_meal_plan(&encoded).unwrap();

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].days_of_week, entries[0].days_of_week);
        assert_eq!(decoded[0].time, entries[0].time);
        assert_eq!(decoded[0].portion, entries[0].portion);
        assert_eq!(decoded[0].status, entries[0].status);
        assert_eq!(decoded[1].days_of_week, entries[1].days_of_week);
        assert_eq!(decoded[1].time, entries[1].time);
        assert_eq!(decoded[1].portion, entries[1].portion);
        assert_eq!(decoded[1].status, entries[1].status);
    }

    #[test]
    fn decode_meal_plan_rejects_invalid_base64() {
        let error = decode_meal_plan("***").unwrap_err();
        assert!(!error.to_string().is_empty());
    }
}
