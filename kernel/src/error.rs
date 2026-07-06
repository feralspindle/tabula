use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("too many requests")]
    TooManyRequests,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("conflict, retries exhausted")]
    Conflict,
    /// plugin trap, invalid manifest, or a decide result the capability
    /// validator rejected, a plugin bug, never the client's fault.
    #[error("plugin error: {0}")]
    Plugin(String),
    /// the plugin evaluated the command and refused it under its game rules.
    /// returned to the issuing client only; nothing is appended to the log.
    #[error("rule error: {0}")]
    Rule(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, self.to_string()),
            AppError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            AppError::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            AppError::Conflict => (StatusCode::CONFLICT, self.to_string()),
            AppError::Rule(_) => (StatusCode::UNPROCESSABLE_ENTITY, self.to_string()),
            AppError::Plugin(e) => {
                tracing::error!("plugin error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal error".to_string(),
                )
            }
            AppError::Database(e) => {
                tracing::error!("database error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal error".to_string(),
                )
            }
        };

        (status, Json(json!({ "message": message }))).into_response()
    }
}
