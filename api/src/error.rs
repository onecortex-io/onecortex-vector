use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    InvalidArgument(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    AlreadyExists(String),
    #[error("{0}")]
    Unauthenticated(String),
    #[error("{0}")]
    PermissionDenied(String),
    #[error("Internal server error")]
    Internal(#[from] anyhow::Error),
    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

#[derive(serde::Serialize)]
struct ErrorBody {
    status: u16,
    error: ErrorDetail,
}

#[derive(serde::Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            ApiError::InvalidArgument(msg) => (StatusCode::BAD_REQUEST, "INVALID_ARGUMENT", msg.clone()),
            ApiError::NotFound(msg)        => (StatusCode::NOT_FOUND, "NOT_FOUND", msg.clone()),
            ApiError::AlreadyExists(msg)   => (StatusCode::CONFLICT, "ALREADY_EXISTS", msg.clone()),
            ApiError::Unauthenticated(msg) => (StatusCode::UNAUTHORIZED, "UNAUTHENTICATED", msg.clone()),
            ApiError::PermissionDenied(msg)=> (StatusCode::FORBIDDEN, "PERMISSION_DENIED", msg.clone()),
            ApiError::Internal(_)          => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", "Internal server error.".to_string()),
            ApiError::Database(_)          => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", "Database error.".to_string()),
        };

        // Log internal errors — don't leak details to clients
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "Internal error");
        }

        let body = ErrorBody {
            status: status.as_u16(),
            error: ErrorDetail { code, message, details: None },
        };
        (status, Json(body)).into_response()
    }
}
