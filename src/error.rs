use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

/// Stable, machine-readable error codes returned in the `error.code` field of
/// every error body. SDKs build typed exception hierarchies on top of these.
#[derive(Debug, Clone, Copy)]
pub enum ErrorCode {
    InvalidArgument,
    NotFound,
    AlreadyExists,
    PermissionDenied,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidArgument => "INVALID_ARGUMENT",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::AlreadyExists => "ALREADY_EXISTS",
            ErrorCode::PermissionDenied => "PERMISSION_DENIED",
            ErrorCode::Internal => "INTERNAL",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{message}")]
    BadRequest {
        code: ErrorCode,
        message: String,
        details: Option<serde_json::Value>,
    },
    #[error("{message}")]
    NotFound {
        code: ErrorCode,
        message: String,
        details: Option<serde_json::Value>,
    },
    #[error("{message}")]
    Conflict {
        code: ErrorCode,
        message: String,
        details: Option<serde_json::Value>,
    },
    #[error("{message}")]
    Forbidden {
        code: ErrorCode,
        message: String,
        details: Option<serde_json::Value>,
    },
    #[error("Internal server error")]
    Internal(#[from] anyhow::Error),
    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl ApiError {
    /// Generic 400 with a free-form message. Prefer typed builders (added in a
    /// later commit) over this one.
    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        ApiError::BadRequest {
            code: ErrorCode::InvalidArgument,
            message: msg.into(),
            details: None,
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        ApiError::NotFound {
            code: ErrorCode::NotFound,
            message: msg.into(),
            details: None,
        }
    }

    pub fn already_exists(msg: impl Into<String>) -> Self {
        ApiError::Conflict {
            code: ErrorCode::AlreadyExists,
            message: msg.into(),
            details: None,
        }
    }

    pub fn permission_denied(msg: impl Into<String>) -> Self {
        ApiError::Forbidden {
            code: ErrorCode::PermissionDenied,
            message: msg.into(),
            details: None,
        }
    }
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
        let (status, code, message, details) = match self {
            ApiError::BadRequest {
                code,
                message,
                details,
            } => (StatusCode::BAD_REQUEST, code.as_str(), message, details),
            ApiError::NotFound {
                code,
                message,
                details,
            } => (StatusCode::NOT_FOUND, code.as_str(), message, details),
            ApiError::Conflict {
                code,
                message,
                details,
            } => (StatusCode::CONFLICT, code.as_str(), message, details),
            ApiError::Forbidden {
                code,
                message,
                details,
            } => (StatusCode::FORBIDDEN, code.as_str(), message, details),
            ApiError::Internal(e) => {
                tracing::error!(error = %e, "Internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorCode::Internal.as_str(),
                    "Internal server error.".to_string(),
                    None,
                )
            }
            ApiError::Database(e) => {
                tracing::error!(error = %e, "Database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorCode::Internal.as_str(),
                    "Database error.".to_string(),
                    None,
                )
            }
        };

        let body = ErrorBody {
            status: status.as_u16(),
            error: ErrorDetail {
                code,
                message,
                details,
            },
        };
        (status, Json(body)).into_response()
    }
}
