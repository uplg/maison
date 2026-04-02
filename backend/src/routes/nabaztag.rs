use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    AppState,
    auth::AuthenticatedUser,
    error::AppError,
    nabaztag::{
        self, EarMoveRequest, ForthRequest, InfoServiceRequest, LedColors,
        NabaztagConfig, PlayUrlRequest, SayRequest, SetupRequest,
    },
};

// ---------------------------------------------------------------------------
// Request / Response DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigResponse {
    success: bool,
    config: NabaztagConfig,
    message: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusResponse {
    success: bool,
    status: nabaztag::NabaztagStatus,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct SimpleResponse {
    success: bool,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EarsResponse {
    success: bool,
    ears: nabaztag::EarPosition,
    message: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ForthResponse {
    success: bool,
    output: String,
    message: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnimationsResponse {
    success: bool,
    animations: serde_json::Value,
    message: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TasksResponse {
    success: bool,
    tasks: serde_json::Value,
    message: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TempoPushResponse {
    success: bool,
    result: nabaztag::TempoPushResult,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TempoPushRequest {
    /// Force refresh Tempo data from RTE before pushing (default: false)
    #[serde(default)]
    force_refresh: bool,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        // Configuration
        .route("/config", get(get_config).post(update_config))
        // Status
        .route("/status", get(get_status))
        // Sleep / Wake
        .route("/sleep", post(sleep))
        .route("/wakeup", post(wakeup))
        // Ears
        .route("/ears", get(get_ears).post(move_ear))
        // LEDs
        .route("/leds", post(set_leds))
        .route("/leds/clear", post(clear_leds))
        // Sound
        .route("/play", post(play_url))
        .route("/say", post(say))
        .route("/sound/communication", post(sound_communication))
        .route("/sound/ack", post(sound_ack))
        .route("/sound/abort", post(sound_abort))
        .route("/sound/ministop", post(sound_ministop))
        .route("/stop", post(stop))
        // Info services (LED animation channels)
        .route("/info", post(set_info_service))
        .route("/info/clear", post(clear_info))
        // Utility
        .route("/taichi", post(taichi))
        .route("/surprise", post(surprise))
        .route("/reboot", post(reboot))
        .route("/update-time", post(update_time))
        .route("/animations", get(get_animations))
        .route("/tasks", get(get_tasks))
        // Setup
        .route("/setup", post(setup))
        // Forth interpreter
        .route("/forth", post(execute_forth))
        // Tempo integration
        .route("/tempo/push", post(push_tempo))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn get_config(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<ConfigResponse>, AppError> {
    let _ = user.0;
    let config = state.nabaztag.get_config().await;
    Ok(Json(ConfigResponse {
        success: true,
        config,
        message: "Nabaztag configuration retrieved",
    }))
}

async fn update_config(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<NabaztagConfig>,
) -> Result<Json<ConfigResponse>, AppError> {
    let _ = user.0;
    if body.host.trim().is_empty() {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "host is required",
        ));
    }
    let config = state.nabaztag.update_config(body).await?;
    Ok(Json(ConfigResponse {
        success: true,
        config,
        message: "Nabaztag configuration updated",
    }))
}

async fn get_status(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<StatusResponse>, AppError> {
    let _ = user.0;
    let status = state.nabaztag.status().await?;
    Ok(Json(StatusResponse {
        success: true,
        status,
        message: "Nabaztag status retrieved",
    }))
}

async fn sleep(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.sleep().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Nabaztag is going to sleep".to_string(),
    }))
}

async fn wakeup(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.wakeup().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Nabaztag is waking up".to_string(),
    }))
}

async fn get_ears(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<EarsResponse>, AppError> {
    let _ = user.0;
    let ears = state.nabaztag.get_ears().await?;
    Ok(Json(EarsResponse {
        success: true,
        ears,
        message: "Ear positions retrieved",
    }))
}

async fn move_ear(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<EarMoveRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    let ear_name = if body.ear == 0 { "left" } else { "right" };
    state
        .nabaztag
        .move_ear(body.ear, body.position, body.direction)
        .await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: format!("Moved {} ear to position {}", ear_name, body.position),
    }))
}

async fn set_leds(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<LedColors>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.set_leds(&body).await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "LED colors set".to_string(),
    }))
}

async fn clear_leds(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.clear_leds().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "LED overrides cleared".to_string(),
    }))
}

async fn play_url(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<PlayUrlRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    if body.url.trim().is_empty() {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "url is required",
        ));
    }
    state.nabaztag.play_url(&body.url).await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: format!("Playing {}", body.url),
    }))
}

async fn say(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<SayRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    if body.text.trim().is_empty() {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "text is required",
        ));
    }
    state.nabaztag.say(&body.text).await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: format!("Saying: {}", body.text),
    }))
}

async fn sound_communication(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.play_midi_communication().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Playing communication sound".to_string(),
    }))
}

async fn sound_ack(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.play_midi_ack().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Playing ack sound".to_string(),
    }))
}

async fn sound_abort(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.play_midi_abort().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Playing abort sound".to_string(),
    }))
}

async fn sound_ministop(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.play_midi_ministop().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Playing ministop sound".to_string(),
    }))
}

async fn stop(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.stop().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Stopped all playback".to_string(),
    }))
}

async fn set_info_service(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<InfoServiceRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state
        .nabaztag
        .set_info_service(&body.service, body.value)
        .await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: format!("Info service '{}' set to {}", body.service, body.value),
    }))
}

async fn clear_info(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.clear_info().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "All info services cleared".to_string(),
    }))
}

async fn taichi(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.taichi().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Tai Chi!".to_string(),
    }))
}

async fn surprise(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.surprise().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Surprise!".to_string(),
    }))
}

async fn reboot(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.reboot().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Nabaztag is rebooting".to_string(),
    }))
}

async fn update_time(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.update_time().await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Time updated from NTP server".to_string(),
    }))
}

async fn get_animations(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<AnimationsResponse>, AppError> {
    let _ = user.0;
    let animations = state.nabaztag.get_animations().await?;
    Ok(Json(AnimationsResponse {
        success: true,
        animations,
        message: "Service animations retrieved",
    }))
}

async fn get_tasks(
    State(state): State<AppState>,
    user: AuthenticatedUser,
) -> Result<Json<TasksResponse>, AppError> {
    let _ = user.0;
    let tasks = state.nabaztag.get_tasks().await?;
    Ok(Json(TasksResponse {
        success: true,
        tasks,
        message: "Nabaztag tasks retrieved",
    }))
}

async fn setup(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<SetupRequest>,
) -> Result<Json<SimpleResponse>, AppError> {
    let _ = user.0;
    state.nabaztag.setup(&body).await?;
    Ok(Json(SimpleResponse {
        success: true,
        message: "Nabaztag setup updated".to_string(),
    }))
}

async fn execute_forth(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Json(body): Json<ForthRequest>,
) -> Result<Json<ForthResponse>, AppError> {
    let _ = user.0;
    if body.code.trim().is_empty() {
        return Err(AppError::http(
            axum::http::StatusCode::BAD_REQUEST,
            "Forth code is required",
        ));
    }
    let result = state.nabaztag.execute_forth(&body.code).await?;
    Ok(Json(ForthResponse {
        success: true,
        output: result.output,
        message: "Forth code executed",
    }))
}

async fn push_tempo(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    body: Option<Json<TempoPushRequest>>,
) -> Result<Json<TempoPushResponse>, AppError> {
    let _ = user.0;
    let force_refresh = body.map(|b| b.force_refresh).unwrap_or(false);

    // Fetch current Tempo data from the TempoService
    let (tempo_data, _) = state.tempo.get_tempo_data(force_refresh).await?;

    let today_color = tempo_data
        .today
        .color
        .as_deref()
        .unwrap_or("UNKNOWN");

    let tomorrow_color = tempo_data.tomorrow.color.as_deref();

    let result = state
        .nabaztag
        .push_tempo(today_color, tomorrow_color)
        .await?;

    Ok(Json(TempoPushResponse {
        success: true,
        message: format!(
            "Tempo pushed: today={}, tomorrow={}",
            result.today_color,
            result.tomorrow_color.as_deref().unwrap_or("unknown")
        ),
        result,
    }))
}
