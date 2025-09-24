use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    BadVersion,
    RoomNotFound,
    RoomFull,
    Unauthorized,
    InvalidTick,
    SlowConsumer,
    RateLimited,
    Internal,
}

#[derive(Debug, Error)]
#[error("{code:?}: {message}")]
pub struct ServerError {
    pub code: ErrorCode,
    pub message: String,
    #[source]
    pub source: Option<anyhow::Error>,
}

impl ServerError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            source: None,
        }
    }

    pub fn with_source(code: ErrorCode, message: impl Into<String>, source: anyhow::Error) -> Self {
        Self {
            code,
            message: message.into(),
            source: Some(source),
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self.code {
            ErrorCode::BadVersion => StatusCode::BAD_REQUEST,
            ErrorCode::RoomNotFound => StatusCode::NOT_FOUND,
            ErrorCode::RoomFull => StatusCode::CONFLICT,
            ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
            ErrorCode::InvalidTick => StatusCode::BAD_REQUEST,
            ErrorCode::SlowConsumer => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    code: ErrorCode,
    message: String,
}

impl IntoResponse for ServerError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status_code();
        let body = Json(ErrorBody {
            code: self.code,
            message: self.message,
        });
        (status, body).into_response()
    }
}
