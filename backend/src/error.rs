use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{message}")]
    Http { status: StatusCode, message: String },
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
    #[error("{0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("{0}")]
    Join(#[from] tokio::task::JoinError),
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    success: bool,
    error: String,
}

impl AppError {
    pub fn http(status: StatusCode, message: impl Into<String>) -> Self {
        Self::Http {
            status,
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::http(StatusCode::UNAUTHORIZED, message)
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::http(StatusCode::SERVICE_UNAVAILABLE, message)
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::Http { status, .. } => *status,
            Self::Io(_) | Self::Json(_) | Self::Jwt(_) | Self::Reqwest(_) | Self::Join(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    fn client_message(&self) -> String {
        match self {
            Self::Http { message, .. } => message.clone(),
            Self::Io(err) => err.to_string(),
            Self::Json(err) => err.to_string(),
            Self::Jwt(err) => err.to_string(),
            Self::Reqwest(err) => err.to_string(),
            Self::Join(err) => err.to_string(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status();
        let body = ErrorBody {
            success: false,
            error: self.client_message(),
        };

        (status, Json(body)).into_response()
    }
}
