use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{AppState, auth::AuthenticatedUser, error::AppError, zigbee};

#[derive(Debug, Serialize)]
struct ZigbeeLampListResponse {
    success: bool,
    lamps: Vec<zigbee::ZigbeeLampView>,
    total: usize,
    connected: usize,
    reachable: usize,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct ZigbeeLampStatsResponse {
    success: bool,
    total: usize,
    connected: usize,
    reachable: usize,
    disabled: bool,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct ZigbeeLampResponse {
    success: bool,
    lamp: Option<zigbee::ZigbeeLampView>,
    message: String,
}

#[derive(Debug, Serialize)]
struct ZigbeeActionResponse {
    success: bool,
    state: zigbee::ZigbeeLampState,
    message: String,
}

#[derive(Debug, Serialize)]
struct ZigbeePairingResponse {
    success: bool,
    pairing: zigbee::ZigbeePairingStatus,
    message: String,
}

#[derive(Debug, Serialize)]
struct ZigbeeTouchlinkResponse {
    success: bool,
    message: String,
}

#[derive(Debug, Deserialize)]
struct PowerBody {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct BrightnessBody {
    brightness: u8,
}

#[derive(Debug, Deserialize)]
struct TemperatureBody {
    temperature: u8,
}

#[derive(Debug, Deserialize)]
struct RenameBody {
    name: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/lamps", get(list_lamps))
        .route("/lamps/stats", get(stats))
        .route("/lamps/pairing/start", post(start_pairing))
        .route("/lamps/pairing/stop", post(stop_pairing))
        .route("/lamps/pairing/status", get(pairing_status))
        .route("/lamps/pairing/touchlink", post(touchlink_scan))
        .route("/lamps/{lamp_id}", get(get_lamp))
        .route("/lamps/{lamp_id}/power", post(set_power))
        .route("/lamps/{lamp_id}/brightness", post(set_brightness))
        .route("/lamps/{lamp_id}/temperature", post(set_temperature))
        .route("/lamps/{lamp_id}/rename", post(rename_lamp))
}

async fn list_lamps(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<ZigbeeLampListResponse>, AppError> {
    let _ = user.0;
    let lamps = state.zigbee.list_lamps().await;
    let total = lamps.len();
    let connected = lamps.iter().filter(|lamp| lamp.connected).count();
    let reachable = lamps.iter().filter(|lamp| lamp.reachable).count();

    Ok(Json(ZigbeeLampListResponse {
        success: true,
        lamps,
        total,
        connected,
        reachable,
        message: "Zigbee lamps list retrieved",
    }))
}

async fn stats(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<ZigbeeLampStatsResponse>, AppError> {
    let _ = user.0;
    let stats = state.zigbee.stats().await;

    Ok(Json(ZigbeeLampStatsResponse {
        success: true,
        total: stats.total,
        connected: stats.connected,
        reachable: stats.reachable,
        disabled: stats.disabled,
        message: stats.message,
    }))
}

async fn get_lamp(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<ZigbeeLampResponse>, AppError> {
    let _ = user.0;
    let lamp = state.zigbee.get_lamp(&lamp_id).await;
    let message = if lamp.is_some() {
        "Zigbee lamp retrieved".to_string()
    } else {
        "Zigbee lamp not found".to_string()
    };

    Ok(Json(ZigbeeLampResponse {
        success: lamp.is_some(),
        lamp,
        message,
    }))
}

async fn pairing_status(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<ZigbeePairingResponse>, AppError> {
    let _ = user.0;
    let pairing = state.zigbee.pairing_status().await;
    let message = if pairing.active {
        "Zigbee pairing is active".to_string()
    } else {
        "Zigbee pairing is inactive".to_string()
    };

    Ok(Json(ZigbeePairingResponse {
        success: true,
        pairing,
        message,
    }))
}

async fn start_pairing(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<ZigbeePairingResponse>, AppError> {
    let _ = user.0;
    let pairing = state.zigbee.start_pairing().await?;
    Ok(Json(ZigbeePairingResponse {
        success: true,
        pairing,
        message: "Zigbee pairing started".to_string(),
    }))
}

async fn stop_pairing(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<ZigbeePairingResponse>, AppError> {
    let _ = user.0;
    let pairing = state.zigbee.stop_pairing().await?;
    Ok(Json(ZigbeePairingResponse {
        success: true,
        pairing,
        message: "Zigbee pairing stopped".to_string(),
    }))
}

async fn touchlink_scan(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<ZigbeeTouchlinkResponse>, AppError> {
    let _ = user.0;
    state.zigbee.touchlink_scan().await?;
    Ok(Json(ZigbeeTouchlinkResponse {
        success: true,
        message: "Touchlink scan initiated".to_string(),
    }))
}

async fn set_power(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<PowerBody>,
) -> Result<Json<ZigbeeActionResponse>, AppError> {
    let _ = user.0;
    let lamp_state = state.zigbee.set_power(&lamp_id, body.enabled).await?;
    Ok(Json(ZigbeeActionResponse {
        success: true,
        state: lamp_state,
        message: "Zigbee lamp power updated".to_string(),
    }))
}

async fn set_brightness(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<BrightnessBody>,
) -> Result<Json<ZigbeeActionResponse>, AppError> {
    let _ = user.0;
    let lamp_state = state.zigbee.set_brightness(&lamp_id, body.brightness).await?;
    Ok(Json(ZigbeeActionResponse {
        success: true,
        state: lamp_state,
        message: "Zigbee lamp brightness updated".to_string(),
    }))
}

async fn set_temperature(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<TemperatureBody>,
) -> Result<Json<ZigbeeActionResponse>, AppError> {
    let _ = user.0;
    let lamp_state = state.zigbee.set_temperature(&lamp_id, body.temperature).await?;
    Ok(Json(ZigbeeActionResponse {
        success: true,
        state: lamp_state,
        message: "Zigbee lamp temperature updated".to_string(),
    }))
}

async fn rename_lamp(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<RenameBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _ = user.0;
    state.zigbee.rename_lamp(&lamp_id, &body.name).await?;
    Ok(Json(serde_json::json!({
        "success": true,
        "message": "Zigbee lamp renamed"
    })))
}
