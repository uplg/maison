use std::{fs, sync::Arc};

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use axum_extra::extract::cookie::{Cookie, SameSite};
use chrono::{Duration, Utc};
use jsonwebtoken::{EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use time::Duration as CookieDuration;
use tracing::{info, warn};

use crate::{
    AppState,
    auth::{AuthUser, Claims, decode_token, extract_auth_token},
    config::Config,
    error::AppError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct User {
    id: String,
    username: String,
    password_hash: String,
    role: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct LegacyUser {
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

pub(crate) fn load_users(config: &Config) -> Result<Vec<User>, AppError> {
    let content = fs::read_to_string(&config.users_path).map_err(|error| {
        AppError::http(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Unable to read users file at {}: {error}",
                config.users_path.display()
            ),
        )
    })?;

    if let Ok(users) = serde_json::from_str::<Vec<User>>(&content) {
        if users.is_empty() {
            return Err(AppError::http(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Users file at {} does not contain any accounts",
                    config.users_path.display()
                ),
            ));
        }

        return Ok(users);
    }

    if serde_json::from_str::<Vec<LegacyUser>>(&content).is_ok() {
        return Err(AppError::http(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Users file at {} still uses plaintext passwords. Rehash them before starting the app.",
                config.users_path.display()
            ),
        ));
    }

    Err(AppError::http(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!(
            "Unable to parse users file at {}. Expected password_hash entries.",
            config.users_path.display()
        ),
    ))
}

async fn login_handler(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    let remote_ip = client_ip(&headers);
    let limiter_key = format!("{remote_ip}:{}", body.username.trim().to_ascii_lowercase());
    let rate_limit = state
        .auth_rate_limiter
        .check(
            &limiter_key,
            state.config.auth_rate_limit_attempts,
            Duration::seconds(state.config.auth_rate_limit_window_seconds),
        )
        .await;

    if !rate_limit.allowed {
        warn!(
            username = %body.username,
            remote_ip = %remote_ip,
            attempts = rate_limit.attempts,
            "auth login rate limit exceeded"
        );
        return Err(AppError::http(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many login attempts. Please wait before retrying.",
        ));
    }

    let user = state
        .users
        .iter()
        .find(|user| user.username == body.username)
        .ok_or_else(|| {
            warn!(username = %body.username, remote_ip = %remote_ip, "auth login failed");
            AppError::http(StatusCode::UNAUTHORIZED, "Invalid username or password")
        })?;

    let parsed_hash = PasswordHash::new(&user.password_hash).map_err(|_| {
        AppError::http(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Stored password hash for user {} is invalid", user.username),
        )
    })?;

    if Argon2::default()
        .verify_password(body.password.as_bytes(), &parsed_hash)
        .is_err()
    {
        warn!(username = %body.username, remote_ip = %remote_ip, "auth login failed");
        return Err(AppError::http(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password",
        ));
    }

    state.auth_rate_limiter.reset(&limiter_key).await;

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

    let cookie = Cookie::build((state.config.auth_cookie_name.clone(), token))
        .http_only(true)
        .secure(state.config.auth_cookie_secure)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(CookieDuration::days(7))
        .build();

    info!(username = %user.username, remote_ip = %remote_ip, "auth login succeeded");

    Ok((
        [(axum::http::header::SET_COOKIE, cookie.to_string())],
        Json(LoginResponse {
            success: true,
            user: AuthUser {
                id: user.id.clone(),
                username: user.username.clone(),
                role: user.role.clone(),
            },
        }),
    ))
}

async fn verify_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<VerifyResponse>, AppError> {
    let token = extract_auth_token(&headers, &state.config.auth_cookie_name)?;
    let decoded = decode_token(token, state.config.jwt_secret.as_bytes())
        .map_err(|_| AppError::http(StatusCode::UNAUTHORIZED, "Invalid token"))?;

    Ok(Json(VerifyResponse {
        success: true,
        user: decoded.claims.into(),
    }))
}

async fn logout_handler(State(state): State<AppState>) -> impl IntoResponse {
    let cookie = Cookie::build((state.config.auth_cookie_name.clone(), String::new()))
        .http_only(true)
        .secure(state.config.auth_cookie_secure)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(CookieDuration::seconds(0))
        .build();

    (
        [(axum::http::header::SET_COOKIE, cookie.to_string())],
        Json(LogoutResponse {
            success: true,
            message: "Logged out successfully",
        }),
    )
}

pub(crate) type SharedUsers = Arc<Vec<User>>;

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .or_else(|| headers.get("x-real-ip").and_then(|value| value.to_str().ok()))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}
