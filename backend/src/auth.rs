use axum::{
    extract::{FromRef, FromRequestParts},
    http::{header::AUTHORIZATION, request::Parts, HeaderMap, StatusCode},
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

use crate::{error::AppError, AppState};

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
        let token = extract_bearer_token(&parts.headers).map_err(|_| {
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
