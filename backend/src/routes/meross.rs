use axum::{
    extract::{Path, State},
    Json, Router,
    routing::{get, post},
};
use serde::Deserialize;
use serde::Serialize;

use crate::{auth::AuthenticatedUser, error::AppError, meross, AppState};

#[derive(Debug, Serialize)]
struct MerossListResponse {
    success: bool,
    devices: Vec<meross::MerossDeviceListEntry>,
    total: usize,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct MerossStatusResponse {
    success: bool,
    device: meross::MerossDeviceRef,
    status: meross::MerossStatus,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct MerossElectricityResponse {
    success: bool,
    device: meross::MerossDeviceRef,
    electricity: meross::MerossElectricityFormatted,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct MerossConsumptionResponse {
    success: bool,
    device: meross::MerossDeviceRef,
    consumption: Vec<meross::MerossConsumptionEntry>,
    summary: meross::MerossConsumptionSummary,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct MerossToggleRouteResponse {
    success: bool,
    device: meross::MerossDeviceRef,
    on: bool,
    message: String,
}

#[derive(Debug, Serialize)]
struct MerossDndRouteResponse {
    success: bool,
    device: meross::MerossDeviceRef,
    #[serde(rename = "dndMode")]
    dnd_mode: bool,
    message: String,
}

#[derive(Debug, Deserialize)]
struct ToggleBody {
    on: bool,
}

#[derive(Debug, Deserialize)]
struct DndBody {
    enabled: bool,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_devices))
        .route("/stats", get(stats))
        .route("/{device_id}/status", get(status))
        .route("/{device_id}/electricity", get(electricity))
        .route("/{device_id}/consumption", get(consumption))
        .route("/{device_id}/toggle", post(toggle))
        .route("/{device_id}/on", post(turn_on))
        .route("/{device_id}/off", post(turn_off))
        .route("/{device_id}/dnd", post(set_dnd))
}

async fn list_devices(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<MerossListResponse>, AppError> {
    let _ = user.0;
    let devices = state.meross.list_devices().await;
    Ok(Json(MerossListResponse {
        success: true,
        total: devices.len(),
        devices,
        message: "Meross devices list retrieved",
    }))
}

async fn stats(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let _ = user.0;
    let stats = state.meross.get_stats().await;
    Ok(Json(serde_json::to_value(stats).map_err(AppError::from)?))
}

async fn status(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<MerossStatusResponse>, AppError> {
    let _ = user.0;
    let (device, status) = state.meross.get_status(&device_id).await?;
    Ok(Json(MerossStatusResponse {
        success: true,
        device,
        status,
        message: "Status retrieved",
    }))
}

async fn electricity(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<MerossElectricityResponse>, AppError> {
    let _ = user.0;
    let (device, electricity) = state.meross.get_electricity(&device_id).await?;
    Ok(Json(MerossElectricityResponse {
        success: true,
        device,
        electricity,
        message: "Electricity data retrieved",
    }))
}

async fn consumption(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<MerossConsumptionResponse>, AppError> {
    let _ = user.0;
    let (device, consumption, summary) = state.meross.get_consumption(&device_id).await?;
    Ok(Json(MerossConsumptionResponse {
        success: true,
        device,
        consumption,
        summary,
        message: "Consumption history retrieved",
    }))
}

async fn toggle(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<ToggleBody>,
) -> Result<Json<MerossToggleRouteResponse>, AppError> {
    let _ = user.0;
    let response = state.meross.toggle(&device_id, body.on).await?;
    Ok(Json(MerossToggleRouteResponse {
        success: true,
        device: response.device,
        on: response.on,
        message: response.message,
    }))
}

async fn turn_on(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<MerossToggleRouteResponse>, AppError> {
    let _ = user.0;
    let response = state.meross.toggle(&device_id, true).await?;
    Ok(Json(MerossToggleRouteResponse {
        success: true,
        device: response.device,
        on: response.on,
        message: response.message,
    }))
}

async fn turn_off(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
) -> Result<Json<MerossToggleRouteResponse>, AppError> {
    let _ = user.0;
    let response = state.meross.toggle(&device_id, false).await?;
    Ok(Json(MerossToggleRouteResponse {
        success: true,
        device: response.device,
        on: response.on,
        message: response.message,
    }))
}

async fn set_dnd(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    user: AuthenticatedUser,
    Json(body): Json<DndBody>,
) -> Result<Json<MerossDndRouteResponse>, AppError> {
    let _ = user.0;
    let response = state.meross.set_dnd(&device_id, body.enabled).await?;
    Ok(Json(MerossDndRouteResponse {
        success: true,
        device: response.device,
        dnd_mode: response.dnd_mode,
        message: response.message,
    }))
}
