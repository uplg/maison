use std::{fs, net::SocketAddr, sync::Arc};

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::{AppendHeaders, IntoResponse},
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
    auth::{AuthUser, Claims, RefreshEntry, decode_token, extract_auth_token},
    config::Config,
    error::AppError,
};

use uuid::Uuid;

/// Cookie name for the refresh token.
const REFRESH_COOKIE_NAME: &str = "maison_refresh";

/// Access token lifetime in minutes.
const ACCESS_TOKEN_MINUTES: i64 = 15;

/// Refresh token lifetime in days (sliding window).
const REFRESH_TOKEN_DAYS: i64 = 7;

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

#[derive(Debug, Serialize)]
struct RefreshResponse {
    success: bool,
    user: AuthUser,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login_handler))
        .route("/verify", post(verify_handler))
        .route("/logout", post(logout_handler))
        .route("/refresh", post(refresh_handler))
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
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    let remote_ip = real_client_ip(&headers, Some(addr));
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
        .find(|user| user.username.eq_ignore_ascii_case(&body.username))
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

    // Issue short-lived access token (15 minutes).
    let access_expiration = Utc::now() + Duration::minutes(ACCESS_TOKEN_MINUTES);
    let claims = Claims {
        user_id: user.id.clone(),
        username: user.username.clone(),
        role: user.role.clone(),
        exp: access_expiration.timestamp() as usize,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.config.jwt_secret.as_bytes()),
    )?;

    let access_cookie = Cookie::build((state.config.auth_cookie_name.clone(), token))
        .http_only(true)
        .secure(state.config.auth_cookie_secure)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(CookieDuration::minutes(ACCESS_TOKEN_MINUTES))
        .build();

    // Issue long-lived refresh token (7-day sliding window).
    let refresh_token = Uuid::new_v4().to_string();
    let refresh_expires_at = (Utc::now() + Duration::days(REFRESH_TOKEN_DAYS)).timestamp();
    state
        .refresh_store
        .insert(
            refresh_token.clone(),
            RefreshEntry {
                user_id: user.id.clone(),
                username: user.username.clone(),
                role: user.role.clone(),
                expires_at: refresh_expires_at,
            },
        )
        .await;

    let refresh_cookie = Cookie::build((REFRESH_COOKIE_NAME.to_string(), refresh_token))
        .http_only(true)
        .secure(state.config.auth_cookie_secure)
        .same_site(SameSite::Lax)
        .path("/api/auth")
        .max_age(CookieDuration::days(REFRESH_TOKEN_DAYS))
        .build();

    info!(username = %user.username, remote_ip = %remote_ip, "auth login succeeded");

    Ok((
        AppendHeaders([
            (axum::http::header::SET_COOKIE, access_cookie.to_string()),
            (axum::http::header::SET_COOKIE, refresh_cookie.to_string()),
        ]),
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

async fn refresh_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    // Extract the refresh token from the cookie.
    let refresh_token = extract_cookie_value(&headers, REFRESH_COOKIE_NAME)
        .ok_or_else(|| AppError::http(StatusCode::UNAUTHORIZED, "No refresh token"))?;

    // Validate and consume the old refresh token.
    let entry = state
        .refresh_store
        .validate(&refresh_token)
        .await
        .ok_or_else(|| AppError::http(StatusCode::UNAUTHORIZED, "Invalid or expired refresh token"))?;

    // Rotate: delete old token immediately.
    state.refresh_store.remove(&refresh_token).await;

    // Issue a new short-lived access token.
    let access_expiration = Utc::now() + Duration::minutes(ACCESS_TOKEN_MINUTES);
    let claims = Claims {
        user_id: entry.user_id.clone(),
        username: entry.username.clone(),
        role: entry.role.clone(),
        exp: access_expiration.timestamp() as usize,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.config.jwt_secret.as_bytes()),
    )
    .map_err(|_| AppError::http(StatusCode::INTERNAL_SERVER_ERROR, "Failed to issue access token"))?;

    let access_cookie = Cookie::build((state.config.auth_cookie_name.clone(), token))
        .http_only(true)
        .secure(state.config.auth_cookie_secure)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(CookieDuration::minutes(ACCESS_TOKEN_MINUTES))
        .build();

    // Issue a new refresh token (rotation + sliding window).
    let new_refresh_token = Uuid::new_v4().to_string();
    let new_refresh_expires_at = (Utc::now() + Duration::days(REFRESH_TOKEN_DAYS)).timestamp();
    state
        .refresh_store
        .insert(
            new_refresh_token.clone(),
            RefreshEntry {
                user_id: entry.user_id.clone(),
                username: entry.username.clone(),
                role: entry.role.clone(),
                expires_at: new_refresh_expires_at,
            },
        )
        .await;

    let refresh_cookie = Cookie::build((REFRESH_COOKIE_NAME.to_string(), new_refresh_token))
        .http_only(true)
        .secure(state.config.auth_cookie_secure)
        .same_site(SameSite::Lax)
        .path("/api/auth")
        .max_age(CookieDuration::days(REFRESH_TOKEN_DAYS))
        .build();

    Ok((
        AppendHeaders([
            (axum::http::header::SET_COOKIE, access_cookie.to_string()),
            (axum::http::header::SET_COOKIE, refresh_cookie.to_string()),
        ]),
        Json(RefreshResponse {
            success: true,
            user: AuthUser {
                id: entry.user_id,
                username: entry.username,
                role: entry.role,
            },
        }),
    ))
}

async fn logout_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Revoke the refresh token so it cannot be reused.
    if let Some(refresh_token) = extract_cookie_value(&headers, REFRESH_COOKIE_NAME) {
        state.refresh_store.remove(&refresh_token).await;
    }

    let access_cookie = Cookie::build((state.config.auth_cookie_name.clone(), String::new()))
        .http_only(true)
        .secure(state.config.auth_cookie_secure)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(CookieDuration::seconds(0))
        .build();

    let refresh_cookie = Cookie::build((REFRESH_COOKIE_NAME.to_string(), String::new()))
        .http_only(true)
        .secure(state.config.auth_cookie_secure)
        .same_site(SameSite::Lax)
        .path("/api/auth")
        .max_age(CookieDuration::seconds(0))
        .build();

    (
        AppendHeaders([
            (axum::http::header::SET_COOKIE, access_cookie.to_string()),
            (axum::http::header::SET_COOKIE, refresh_cookie.to_string()),
        ]),
        Json(LogoutResponse {
            success: true,
            message: "Logged out successfully",
        }),
    )
}

/// Determine the real client IP address from trusted proxy headers with
/// a fallback to the TCP peer address.
///
/// Resolution order:
/// 1. `CF-Connecting-IP` — set by Cloudflare, trustworthy in production.
/// 2. First entry of `X-Forwarded-For` — set by most reverse proxies.
/// 3. TCP peer address via `ConnectInfo<SocketAddr>` (may be `None` if the
///    server was not configured with `into_make_service_with_connect_info`).
/// 4. `"unknown"` as a final fallback so the rate limiter still works
///    (all unknown clients share a single bucket, which is conservative).
fn real_client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
    if let Some(cf_ip) = headers
        .get("CF-Connecting-IP")
        .and_then(|v| v.to_str().ok())
    {
        return cf_ip.trim().to_string();
    }

    if let Some(xff) = headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
    {
        if let Some(first) = xff.split(',').next() {
            let trimmed = first.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    peer.map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Extract a named cookie value from the request headers.
fn extract_cookie_value<'a>(headers: &'a HeaderMap, cookie_name: &str) -> Option<&'a str> {
    headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie_header| {
            cookie_header
                .split(';')
                .filter_map(|cookie| cookie.trim().split_once('='))
                .find_map(|(name, value)| (name == cookie_name).then_some(value))
        })
}

pub(crate) type SharedUsers = Arc<Vec<User>>;
