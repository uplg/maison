use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde::Serialize;

use crate::{
    auth::AuthenticatedUser,
    error::AppError,
    tempo::{
        TempoCalibrationResponse, TempoCalendarResponse, TempoData, TempoHistoryResponse,
        TempoPredictionServiceResponse, TempoPredictionState, TempoTarifs,
    },
    AppState,
};

#[derive(Debug, Serialize)]
struct TempoDataResponse {
    success: bool,
    today: crate::tempo::TempoDay,
    tomorrow: crate::tempo::TempoDay,
    tarifs: Option<TempoTarifs>,
    #[serde(rename = "lastUpdated")]
    last_updated: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cached: Option<bool>,
    message: String,
}

#[derive(Debug, Serialize)]
struct TempoErrorResponse {
    success: bool,
    error: String,
    message: String,
}

#[derive(Debug, Serialize)]
struct TempoPredictionsResponse {
    success: bool,
    predictions: Vec<crate::tempo::TempoPrediction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<TempoPredictionState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_version: Option<String>,
    message: String,
}

#[derive(Debug, Serialize)]
struct TempoStateResponse {
    success: bool,
    season: String,
    stock_red_remaining: i32,
    stock_red_total: i32,
    stock_white_remaining: i32,
    stock_white_total: i32,
    consecutive_red: i32,
    message: String,
}

#[derive(Debug, Deserialize)]
struct SeasonQuery {
    season: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_tempo))
        .route("/refresh", post(refresh_tempo))
        .route("/predictions", get(get_predictions))
        .route("/state", get(get_state))
        .route("/calendar", get(get_calendar))
        .route("/history", get(get_history))
        .route("/calibration", get(get_calibration))
}

async fn get_tempo(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> impl IntoResponse {
    let _ = user.0;

    match state.tempo.get_tempo_data(false).await {
        Ok((data, is_cached)) => ok_tempo_response(
            data,
            if is_cached {
                "Returning cached Tempo data (API unavailable)"
            } else {
                "Tempo data retrieved successfully"
            },
            is_cached,
        ),
        Err(error) => tempo_service_error(error, "Tempo service unavailable"),
    }
}

async fn refresh_tempo(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> impl IntoResponse {
    let _ = user.0;

    match state.tempo.get_tempo_data(true).await {
        Ok((data, _)) => ok_tempo_response(data, "Tempo data refreshed successfully", false),
        Err(error) => tempo_service_error(error, "Tempo service unavailable"),
    }
}

async fn get_predictions(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<TempoPredictionsResponse>, AppError> {
    let _ = user.0;
    let payload: TempoPredictionServiceResponse = state.tempo.get_predictions().await?;

    Ok(Json(TempoPredictionsResponse {
        success: true,
        predictions: payload.predictions,
        state: payload.state,
        model_version: payload.model_version,
        message: "Tempo predictions retrieved successfully".to_string(),
    }))
}

async fn get_state(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<TempoStateResponse>, AppError> {
    let _ = user.0;
    let payload = state.tempo.get_state().await?;

    Ok(Json(TempoStateResponse {
        success: true,
        season: payload.season,
        stock_red_remaining: payload.stock_red_remaining,
        stock_red_total: payload.stock_red_total,
        stock_white_remaining: payload.stock_white_remaining,
        stock_white_total: payload.stock_white_total,
        consecutive_red: payload.consecutive_red,
        message: "Tempo state retrieved successfully".to_string(),
    }))
}

async fn get_calendar(
    State(state): State<AppState>,
    Query(query): Query<SeasonQuery>,
    user: AuthenticatedUser,
) -> Result<Json<TempoCalendarResponse>, AppError> {
    let _ = user.0;
    let response = state.tempo.get_calendar(query.season.as_deref()).await?;
    Ok(Json(response))
}

async fn get_history(
    State(state): State<AppState>,
    Query(query): Query<SeasonQuery>,
    user: AuthenticatedUser,
) -> Result<Json<TempoHistoryResponse>, AppError> {
    let _ = user.0;
    let response = state.tempo.get_history(query.season.as_deref()).await?;
    Ok(Json(response))
}

async fn get_calibration(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<TempoCalibrationResponse>, AppError> {
    let _ = user.0;
    let response = state.tempo.get_calibration().await?;
    Ok(Json(response))
}

fn ok_tempo_response(data: TempoData, message: &str, is_cached: bool) -> axum::response::Response {
    Json(TempoDataResponse {
        success: true,
        today: data.today,
        tomorrow: data.tomorrow,
        tarifs: data.tarifs,
        last_updated: data.last_updated,
        cached: if is_cached { Some(true) } else { None },
        message: message.to_string(),
    })
    .into_response()
}

fn tempo_service_error(error: AppError, message: &str) -> axum::response::Response {
    tracing::warn!(error = %error, "tempo request failed");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(TempoErrorResponse {
            success: false,
            error: error.to_string(),
            message: message.to_string(),
        }),
    )
        .into_response()
}
