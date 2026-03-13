use std::{fs, sync::Arc};

use axum::{
    extract::State,
    http::StatusCode,
    routing::post,
    Json, Router,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{decode_token, extract_bearer_token, AuthUser, Claims},
    config::Config,
    error::AppError,
    AppState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct User {
    id: String,
    username: String,
    password: String,
    role: String,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    success: bool,
    token: String,
    user: AuthUser,
}

#[derive(Debug, Serialize)]
struct VerifyResponse {
    success: bool,
    user: AuthUser,
}

#[derive(Debug, Serialize)]
struct LogoutResponse {
    success: bool,
    message: &'static str,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login_handler))
        .route("/verify", post(verify_handler))
        .route("/logout", post(logout_handler))
}

pub(crate) fn load_users(config: &Config) -> Vec<User> {
    let fallback = vec![User {
        id: "1".to_string(),
        username: "admin".to_string(),
        password: "admin".to_string(),
        role: "admin".to_string(),
    }];

    let Ok(content) = fs::read_to_string(&config.users_path) else {
        return fallback;
    };

    serde_json::from_str::<Vec<User>>(&content).unwrap_or(fallback)
}

async fn login_handler(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let user = state
        .users
        .iter()
        .find(|user| user.username == body.username && user.password == body.password)
        .ok_or_else(|| AppError::http(StatusCode::UNAUTHORIZED, "Invalid username or password"))?;

    let expiration = Utc::now() + Duration::days(7);
    let claims = Claims {
        user_id: user.id.clone(),
        username: user.username.clone(),
        role: user.role.clone(),
        exp: expiration.timestamp() as usize,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.config.jwt_secret.as_bytes()),
    )?;

    Ok(Json(LoginResponse {
        success: true,
        token,
        user: AuthUser {
            id: user.id.clone(),
            username: user.username.clone(),
            role: user.role.clone(),
        },
    }))
}

async fn verify_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<VerifyResponse>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let decoded = decode_token(token, state.config.jwt_secret.as_bytes())
        .map_err(|_| AppError::http(StatusCode::UNAUTHORIZED, "Invalid token"))?;

    Ok(Json(VerifyResponse {
        success: true,
        user: decoded.claims.into(),
    }))
}

async fn logout_handler() -> Json<LogoutResponse> {
    Json(LogoutResponse {
        success: true,
        message: "Logged out successfully",
    })
}

pub(crate) type SharedUsers = Arc<Vec<User>>;
