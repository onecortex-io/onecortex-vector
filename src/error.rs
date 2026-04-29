use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

/// Stable, machine-readable error codes returned in the `error.code` field of
/// every error body. SDKs build typed exception hierarchies on top of these.
// New variants beyond the original five are wired in subsequent commits;
// allow dead_code until then so the bin target compiles cleanly.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum ErrorCode {
    InvalidArgument,
    NotFound,
    AlreadyExists,
    PermissionDenied,
    Internal,
    DimensionMismatch,
    SparseNotSupported,
    FilterMalformed,
    FilterUnsupportedOperator,
    HybridRequiresBm25,
    GroupbyFieldMissing,
    FacetFieldInvalid,
    IndexNotReady,
    CollectionNotFound,
    CollectionAlreadyExists,
    RerankerRateLimited,
    RerankerTimeout,
    RerankerConfig,
    RerankerUpstream,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidArgument => "INVALID_ARGUMENT",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::AlreadyExists => "ALREADY_EXISTS",
            ErrorCode::PermissionDenied => "PERMISSION_DENIED",
            ErrorCode::Internal => "INTERNAL",
            ErrorCode::DimensionMismatch => "DIMENSION_MISMATCH",
            ErrorCode::SparseNotSupported => "SPARSE_NOT_SUPPORTED",
            ErrorCode::FilterMalformed => "FILTER_MALFORMED",
            ErrorCode::FilterUnsupportedOperator => "FILTER_UNSUPPORTED_OPERATOR",
            ErrorCode::HybridRequiresBm25 => "HYBRID_REQUIRES_BM25",
            ErrorCode::GroupbyFieldMissing => "GROUPBY_FIELD_MISSING",
            ErrorCode::FacetFieldInvalid => "FACET_FIELD_INVALID",
            ErrorCode::IndexNotReady => "INDEX_NOT_READY",
            ErrorCode::CollectionNotFound => "COLLECTION_NOT_FOUND",
            ErrorCode::CollectionAlreadyExists => "COLLECTION_ALREADY_EXISTS",
            ErrorCode::RerankerRateLimited => "RERANKER_RATE_LIMITED",
            ErrorCode::RerankerTimeout => "RERANKER_TIMEOUT",
            ErrorCode::RerankerConfig => "RERANKER_CONFIG",
            ErrorCode::RerankerUpstream => "RERANKER_UPSTREAM",
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

    // The typed builders below are wired into call sites in later commits.
    // Allow dead_code in the interim so the bin target compiles cleanly.
    #[allow(dead_code)]
    pub fn dimension_mismatch(record_id: Option<&str>, expected: usize, got: usize) -> Self {
        let message = match record_id {
            Some(id) => {
                format!("record '{id}' has {got} dimensions but collection expects {expected}")
            }
            None => format!("vector has {got} dimensions but collection expects {expected}"),
        };
        let details = match record_id {
            Some(id) => serde_json::json!({ "recordId": id, "expected": expected, "got": got }),
            None => serde_json::json!({ "expected": expected, "got": got }),
        };
        ApiError::BadRequest {
            code: ErrorCode::DimensionMismatch,
            message,
            details: Some(details),
        }
    }

    #[allow(dead_code)]
    pub fn sparse_not_supported(record_id: &str) -> Self {
        ApiError::BadRequest {
            code: ErrorCode::SparseNotSupported,
            message: format!(
                "record '{record_id}' includes sparseValues, which is not supported by this server"
            ),
            details: Some(serde_json::json!({ "recordId": record_id })),
        }
    }

    #[allow(dead_code)]
    pub fn filter_malformed(msg: impl Into<String>) -> Self {
        let message = msg.into();
        ApiError::BadRequest {
            code: ErrorCode::FilterMalformed,
            details: Some(serde_json::json!({ "reason": message })),
            message,
        }
    }

    #[allow(dead_code)]
    pub fn filter_unsupported_operator(op: impl Into<String>) -> Self {
        let op = op.into();
        ApiError::BadRequest {
            code: ErrorCode::FilterUnsupportedOperator,
            message: format!("unsupported filter operator: {op}"),
            details: Some(serde_json::json!({ "operator": op })),
        }
    }

    #[allow(dead_code)]
    pub fn hybrid_requires_bm25(collection: &str) -> Self {
        ApiError::BadRequest {
            code: ErrorCode::HybridRequiresBm25,
            message: format!(
                "hybrid search requires bm25Enabled=true on collection '{collection}'. \
                 Use PATCH /v1/collections/{collection} to enable it."
            ),
            details: Some(serde_json::json!({ "collection": collection })),
        }
    }

    #[allow(dead_code)]
    pub fn groupby_field_missing(field: &str) -> Self {
        ApiError::BadRequest {
            code: ErrorCode::GroupbyFieldMissing,
            message: format!(
                "groupBy.field '{field}' was not present on any matched record's metadata"
            ),
            details: Some(serde_json::json!({ "field": field })),
        }
    }

    #[allow(dead_code)]
    pub fn facet_field_invalid(field: &str, reason: &str) -> Self {
        ApiError::BadRequest {
            code: ErrorCode::FacetFieldInvalid,
            message: format!("facet field '{field}' is invalid: {reason}"),
            details: Some(serde_json::json!({ "field": field, "reason": reason })),
        }
    }

    #[allow(dead_code)]
    pub fn index_not_ready(collection: &str, status: &str) -> Self {
        ApiError::Conflict {
            code: ErrorCode::IndexNotReady,
            message: format!(
                "collection '{collection}' is not ready (status: {status}); retry shortly"
            ),
            details: Some(serde_json::json!({ "collection": collection, "status": status })),
        }
    }

    #[allow(dead_code)]
    pub fn collection_not_found(name: &str) -> Self {
        ApiError::NotFound {
            code: ErrorCode::CollectionNotFound,
            message: format!("collection '{name}' does not exist"),
            details: Some(serde_json::json!({ "collection": name })),
        }
    }

    #[allow(dead_code)]
    pub fn collection_already_exists(name: &str) -> Self {
        ApiError::Conflict {
            code: ErrorCode::CollectionAlreadyExists,
            message: format!("collection '{name}' already exists"),
            details: Some(serde_json::json!({ "collection": name })),
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
        let (status, code, message, mut details) = match self {
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

        // Inject the request id (set by `RequestIdTaskLocalLayer`) into
        // `details.requestId` so SDKs and operators can correlate against
        // server logs. Returns `None` outside a request scope, in which
        // case the field is simply omitted.
        if let Some(request_id) = crate::middleware::request_id::current() {
            let entry = details.get_or_insert_with(|| serde_json::json!({}));
            if let Some(obj) = entry.as_object_mut() {
                obj.insert(
                    "requestId".to_string(),
                    serde_json::Value::String(request_id),
                );
            }
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    async fn render(err: ApiError) -> (StatusCode, serde_json::Value) {
        let response = err.into_response();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, body)
    }

    #[tokio::test]
    async fn dimension_mismatch_with_record_id() {
        let (status, body) = render(ApiError::dimension_mismatch(Some("r42"), 1536, 768)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["status"], 400);
        assert_eq!(body["error"]["code"], "DIMENSION_MISMATCH");
        assert_eq!(body["error"]["details"]["recordId"], "r42");
        assert_eq!(body["error"]["details"]["expected"], 1536);
        assert_eq!(body["error"]["details"]["got"], 768);
    }

    #[tokio::test]
    async fn dimension_mismatch_without_record_id() {
        let (_, body) = render(ApiError::dimension_mismatch(None, 1536, 8)).await;
        assert_eq!(body["error"]["code"], "DIMENSION_MISMATCH");
        assert!(body["error"]["details"].get("recordId").is_none());
    }

    #[tokio::test]
    async fn sparse_not_supported() {
        let (status, body) = render(ApiError::sparse_not_supported("r1")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "SPARSE_NOT_SUPPORTED");
        assert_eq!(body["error"]["details"]["recordId"], "r1");
    }

    #[tokio::test]
    async fn filter_malformed() {
        let (status, body) = render(ApiError::filter_malformed("$and must be an array")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "FILTER_MALFORMED");
        assert_eq!(body["error"]["details"]["reason"], "$and must be an array");
    }

    #[tokio::test]
    async fn filter_unsupported_operator() {
        let (status, body) = render(ApiError::filter_unsupported_operator("$weird")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "FILTER_UNSUPPORTED_OPERATOR");
        assert_eq!(body["error"]["details"]["operator"], "$weird");
    }

    #[tokio::test]
    async fn hybrid_requires_bm25() {
        let (status, body) = render(ApiError::hybrid_requires_bm25("docs")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "HYBRID_REQUIRES_BM25");
        assert_eq!(body["error"]["details"]["collection"], "docs");
    }

    #[tokio::test]
    async fn groupby_field_missing() {
        let (status, body) = render(ApiError::groupby_field_missing("category")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "GROUPBY_FIELD_MISSING");
        assert_eq!(body["error"]["details"]["field"], "category");
    }

    #[tokio::test]
    async fn facet_field_invalid() {
        let (status, body) = render(ApiError::facet_field_invalid(
            "1bad",
            "must start with letter or underscore",
        ))
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "FACET_FIELD_INVALID");
        assert_eq!(body["error"]["details"]["field"], "1bad");
    }

    #[tokio::test]
    async fn index_not_ready() {
        let (status, body) = render(ApiError::index_not_ready("docs", "indexing")).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "INDEX_NOT_READY");
        assert_eq!(body["error"]["details"]["status"], "indexing");
    }

    #[tokio::test]
    async fn collection_not_found() {
        let (status, body) = render(ApiError::collection_not_found("docs")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "COLLECTION_NOT_FOUND");
        assert_eq!(body["error"]["details"]["collection"], "docs");
    }

    #[tokio::test]
    async fn collection_already_exists() {
        let (status, body) = render(ApiError::collection_already_exists("docs")).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "COLLECTION_ALREADY_EXISTS");
        assert_eq!(body["error"]["details"]["collection"], "docs");
    }

    #[tokio::test]
    async fn legacy_invalid_argument_unchanged() {
        let (status, body) = render(ApiError::invalid_argument("nope")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "INVALID_ARGUMENT");
        assert!(body["error"].get("details").is_none());
    }
}
