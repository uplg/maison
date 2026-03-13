use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{AppState, auth::AuthenticatedUser, error::AppError, hue};

#[derive(Debug, Serialize)]
struct HueListResponse {
    success: bool,
    lamps: Vec<hue::HueLampView>,
    total: usize,
    connected: usize,
    reachable: usize,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct HueStatsResponse {
    success: bool,
    total: usize,
    connected: usize,
    reachable: usize,
    disabled: bool,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct HueLampResponse {
    success: bool,
    lamp: Option<hue::HueLampView>,
    message: String,
}

#[derive(Debug, Serialize)]
struct HueActionResponse {
    success: bool,
    state: hue::HueLampState,
    message: String,
}

#[derive(Debug, Serialize)]
struct HueSimpleResponse {
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
struct StateBody {
    #[serde(rename = "isOn")]
    is_on: bool,
    brightness: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct RenameBody {
    name: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_lamps))
        .route("/stats", get(stats))
        .route("/scan", post(scan))
        .route("/connect", post(connect_all))
        .route("/disconnect", post(disconnect_all))
        .route("/{lamp_id}", get(get_lamp))
        .route("/{lamp_id}/connect", post(connect_lamp))
        .route("/{lamp_id}/disconnect", post(disconnect_lamp))
        .route("/{lamp_id}/power", post(set_power))
        .route("/{lamp_id}/brightness", post(set_brightness))
        .route("/{lamp_id}/temperature", post(set_temperature))
        .route("/{lamp_id}/state", post(set_state))
        .route("/{lamp_id}/rename", post(rename_lamp))
        .route("/{lamp_id}/blacklist", post(blacklist_lamp))
}

async fn list_lamps(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<HueListResponse>, AppError> {
    let _ = user.0;
    let lamps = state.hue.list_lamps().await;
    let total = lamps.len();
    let connected = lamps.iter().filter(|lamp| lamp.connected).count();
    let reachable = lamps.iter().filter(|lamp| lamp.reachable).count();
    Ok(Json(HueListResponse {
        success: true,
        lamps,
        total,
        connected,
        reachable,
        message: "Hue lamps list retrieved",
    }))
}

async fn stats(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<HueStatsResponse>, AppError> {
    let _ = user.0;
    let stats = state.hue.stats().await;
    Ok(Json(HueStatsResponse {
        success: true,
        total: stats.total,
        connected: stats.connected,
        reachable: stats.reachable,
        disabled: stats.disabled,
        message: stats.message,
    }))
}

async fn scan(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<HueSimpleResponse>, AppError> {
    let _ = user.0;
    state.hue.trigger_scan().await?;
    Ok(Json(HueSimpleResponse {
        success: true,
        message: "Hue lamp scan started".to_string(),
    }))
}

async fn connect_all(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<HueSimpleResponse>, AppError> {
    let _ = user.0;
    state.hue.connect_all().await;
    Ok(Json(HueSimpleResponse {
        success: true,
        message: "Hue lamp connections started".to_string(),
    }))
}

async fn disconnect_all(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<HueSimpleResponse>, AppError> {
    let _ = user.0;
    state.hue.disconnect_all().await;
    Ok(Json(HueSimpleResponse {
        success: true,
        message: "Hue lamps disconnected".to_string(),
    }))
}

async fn get_lamp(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<HueLampResponse>, AppError> {
    let _ = user.0;
    let lamp = state.hue.get_lamp(&lamp_id).await;
    let message = if lamp.is_some() {
        "Hue lamp retrieved".to_string()
    } else {
        "Hue lamp not found".to_string()
    };
    Ok(Json(HueLampResponse {
        success: lamp.is_some(),
        lamp,
        message,
    }))
}

async fn connect_lamp(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<HueSimpleResponse>, AppError> {
    let _ = user.0;
    let connected = state.hue.connect_lamp(&lamp_id).await?;
    Ok(Json(HueSimpleResponse {
        success: connected,
        message: if connected {
            "Hue lamp connected".to_string()
        } else {
            "Hue lamp connection unavailable".to_string()
        },
    }))
}

async fn disconnect_lamp(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<HueSimpleResponse>, AppError> {
    let _ = user.0;
    state.hue.disconnect_lamp(&lamp_id).await?;
    Ok(Json(HueSimpleResponse {
        success: true,
        message: "Hue lamp disconnected".to_string(),
    }))
}

async fn set_power(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<PowerBody>,
) -> Result<Json<HueActionResponse>, AppError> {
    let _ = user.0;
    let lamp_state = state.hue.set_power(&lamp_id, body.enabled).await?;
    Ok(Json(HueActionResponse {
        success: true,
        state: lamp_state,
        message: "Hue lamp power updated".to_string(),
    }))
}

async fn set_brightness(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<BrightnessBody>,
) -> Result<Json<HueActionResponse>, AppError> {
    let _ = user.0;
    let lamp_state = state.hue.set_brightness(&lamp_id, body.brightness).await?;
    Ok(Json(HueActionResponse {
        success: true,
        state: lamp_state,
        message: "Hue lamp brightness updated".to_string(),
    }))
}

async fn set_temperature(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<TemperatureBody>,
) -> Result<Json<HueActionResponse>, AppError> {
    let _ = user.0;
    let lamp_state = state.hue.set_temperature(&lamp_id, body.temperature).await?;
    Ok(Json(HueActionResponse {
        success: true,
        state: lamp_state,
        message: "Hue lamp temperature updated".to_string(),
    }))
}

async fn set_state(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<StateBody>,
) -> Result<Json<HueActionResponse>, AppError> {
    let _ = user.0;
    let lamp_state = state
        .hue
        .set_lamp_state(&lamp_id, body.is_on, body.brightness)
        .await?;
    Ok(Json(HueActionResponse {
        success: true,
        state: lamp_state,
        message: "Hue lamp state updated".to_string(),
    }))
}

async fn rename_lamp(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<RenameBody>,
) -> Result<Json<HueSimpleResponse>, AppError> {
    let _ = user.0;
    state.hue.rename_lamp(&lamp_id, &body.name).await?;
    Ok(Json(HueSimpleResponse {
        success: true,
        message: "Hue lamp renamed".to_string(),
    }))
}

async fn blacklist_lamp(
    State(state): State<AppState>,
    Path(lamp_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<HueSimpleResponse>, AppError> {
    let _ = user.0;
    let success = state.hue.blacklist_lamp(&lamp_id).await?;
    Ok(Json(HueSimpleResponse {
        success,
        message: if success {
            "Hue lamp blacklisted".to_string()
        } else {
            "Hue lamp not found".to_string()
        },
    }))
}
