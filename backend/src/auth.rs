use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{FromRef, FromRequestParts},
    http::{header::AUTHORIZATION, request::Parts, HeaderMap, StatusCode},
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{error::AppError, AppState};

#[derive(Clone, Default)]
pub struct AuthRateLimiter {
    inner: Arc<Mutex<HashMap<String, RateLimitEntry>>>,
}

/// Server-side store for refresh tokens.
/// Maps opaque token strings to their associated user data and expiration.
/// Expired entries are evicted on each lookup to prevent unbounded growth.
#[derive(Clone, Default)]
pub struct RefreshTokenStore {
    inner: Arc<Mutex<HashMap<String, RefreshEntry>>>,
}

#[derive(Debug, Clone)]
pub struct RefreshEntry {
    pub user_id: String,
    pub username: String,
    pub role: String,
    /// Seconds since epoch when this refresh token expires.
    pub expires_at: i64,
}

#[derive(Debug, Clone)]
struct RateLimitEntry {
    window_started_at: chrono::DateTime<Utc>,
    attempts: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct AuthRateLimitStatus {
    pub allowed: bool,
    pub attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub exp: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub role: String,
}

impl From<Claims> for AuthUser {
    fn from(value: Claims) -> Self {
        Self {
            id: value.user_id,
            username: value.username,
            role: value.role,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthenticatedUser(pub AuthUser);

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    AppState: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);
        let token = extract_auth_token(&parts.headers, &app_state.config.auth_cookie_name).map_err(|_| {
            AppError::http(
                StatusCode::UNAUTHORIZED,
                "Authentication required. Please provide a valid Bearer token.",
            )
        })?;

        let decoded = decode_token(token, app_state.config.jwt_secret.as_bytes())
            .map_err(|_| AppError::http(StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;

        Ok(Self(decoded.claims.into()))
    }
}

/// Extractor that requires the authenticated user to have the `admin` role.
/// Use this for destructive or sensitive operations (device control, settings, etc.).
#[derive(Debug, Clone)]
pub struct AdminUser(pub AuthUser);

impl<S> FromRequestParts<S> for AdminUser
where
    AppState: axum::extract::FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let user = AuthenticatedUser::from_request_parts(parts, state).await?;
        if user.0.role != "admin" {
            return Err(AppError::http(
                StatusCode::FORBIDDEN,
                "Admin privileges required",
            ));
        }
        Ok(Self(user.0))
    }
}

pub fn extract_auth_token<'a>(headers: &'a HeaderMap, cookie_name: &str) -> Result<&'a str, AppError> {
    extract_bearer_token(headers).or_else(|_| extract_cookie_token(headers, cookie_name))
}

pub fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, AppError> {
    let Some(header) = headers.get(AUTHORIZATION).and_then(|value| value.to_str().ok()) else {
        return Err(AppError::unauthorized("No token provided"));
    };

    header
        .strip_prefix("Bearer ")
        .ok_or_else(|| AppError::unauthorized("No token provided"))
}

pub fn decode_token(
    token: &str,
    secret: &[u8],
) -> Result<jsonwebtoken::TokenData<Claims>, jsonwebtoken::errors::Error> {
    decode::<Claims>(token, &DecodingKey::from_secret(secret), &Validation::default())
}

fn extract_cookie_token<'a>(headers: &'a HeaderMap, cookie_name: &str) -> Result<&'a str, AppError> {
    let cookie_header = headers
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::unauthorized("No token provided"))?;

    cookie_header
        .split(';')
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find_map(|(name, value)| (name == cookie_name).then_some(value))
        .ok_or_else(|| AppError::unauthorized("No token provided"))
}

impl AuthRateLimiter {
    pub async fn check(&self, key: &str, max_attempts: u32, window: Duration) -> AuthRateLimitStatus {
        let now = Utc::now();
        let mut entries = self.inner.lock().await;

        // Evict expired entries to prevent unbounded memory growth.
        entries.retain(|_, entry| now - entry.window_started_at < window);

        let entry = entries.entry(key.to_string()).or_insert(RateLimitEntry {
            window_started_at: now,
            attempts: 0,
        });

        if now - entry.window_started_at >= window {
            entry.window_started_at = now;
            entry.attempts = 0;
        }

        entry.attempts = entry.attempts.saturating_add(1);

        AuthRateLimitStatus {
            allowed: entry.attempts <= max_attempts,
            attempts: entry.attempts,
        }
    }

    pub async fn reset(&self, key: &str) {
        self.inner.lock().await.remove(key);
    }
}

impl RefreshTokenStore {
    /// Store a refresh token with its associated user data.
    pub async fn insert(&self, token: String, entry: RefreshEntry) {
        let mut map = self.inner.lock().await;
        map.insert(token, entry);
    }

    /// Look up a refresh token. Returns `None` if the token doesn't exist or is expired.
    /// Evicts all expired entries on each call.
    pub async fn validate(&self, token: &str) -> Option<RefreshEntry> {
        let now = Utc::now().timestamp();
        let mut map = self.inner.lock().await;
        // Evict expired entries to prevent unbounded growth.
        map.retain(|_, entry| entry.expires_at > now);
        map.get(token).cloned()
    }

    /// Remove a refresh token (used on logout and rotation).
    pub async fn remove(&self, token: &str) {
        self.inner.lock().await.remove(token);
    }
}
