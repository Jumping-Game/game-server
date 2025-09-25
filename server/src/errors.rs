use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    Unauthorized,
    BadVersion,
    NotMaster,
    RoomStateInvalid,
    RoomNotReady,
    StartAlready,
    CountdownActive,
    SlowConsumer,
    InvalidState,
    Internal,
}

impl ErrorCode {
    pub fn http_status(&self) -> StatusCode {
        match self {
            ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
            ErrorCode::BadVersion => StatusCode::BAD_REQUEST,
            ErrorCode::NotMaster => StatusCode::FORBIDDEN,
            ErrorCode::RoomStateInvalid => StatusCode::CONFLICT,
            ErrorCode::RoomNotReady => StatusCode::CONFLICT,
            ErrorCode::StartAlready => StatusCode::CONFLICT,
            ErrorCode::CountdownActive => StatusCode::CONFLICT,
            ErrorCode::SlowConsumer => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::InvalidState => StatusCode::BAD_REQUEST,
            ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::Unauthorized => "UNAUTHORIZED",
            ErrorCode::BadVersion => "BAD_VERSION",
            ErrorCode::NotMaster => "NOT_MASTER",
            ErrorCode::RoomStateInvalid => "ROOM_STATE_INVALID",
            ErrorCode::RoomNotReady => "ROOM_NOT_READY",
            ErrorCode::StartAlready => "START_ALREADY",
            ErrorCode::CountdownActive => "COUNTDOWN_ACTIVE",
            ErrorCode::SlowConsumer => "SLOW_CONSUMER",
            ErrorCode::InvalidState => "INVALID_STATE",
            ErrorCode::Internal => "INTERNAL",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireError {
    pub code: String,
    pub message: String,
}

impl WireError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: code.as_str().to_string(),
            message: message.into(),
        }
    }
}

#[derive(Error, Debug)]
pub enum AppError {
    #[error("{code:?}: {message}")]
    Http { code: ErrorCode, message: String },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl AppError {
    pub fn http(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Http {
            code,
            message: message.into(),
        }
    }

    pub fn wire(&self) -> WireError {
        match self {
            AppError::Http { code, message } => WireError::new(code.clone(), message.clone()),
            AppError::Other(err) => WireError::new(ErrorCode::Internal, err.to_string()),
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            AppError::Http { code, .. } => code.http_status(),
            AppError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

impl From<AppError> for axum::http::StatusCode {
    fn from(value: AppError) -> Self {
        value.status()
    }
}

pub type SharedError = Arc<AppError>;

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status();
        let body = Json(self.wire());
        (status, body).into_response()
    }
}
