use crate::types::Id;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Internal server error: {0}")]
    Anyhow(#[from] anyhow::Error),

    #[error("Bad request -- serialization error: {0}")]
    SerializationError(String),

    #[error("Invalid ID: {0}")]
    InvalidId(String),

    #[error("ID already registered: {0}")]
    IdAlreadyRegistered(Id),

    #[error("ID not found: {0}")]
    IdNotFound(Id),
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorResponseBody {
    message: String,
}

impl From<Error> for (StatusCode, ErrorResponseBody) {
    fn from(value: Error) -> Self {
        let message = format!("{}", value);
        let status = match value {
            Error::Anyhow(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::InvalidId(_) | Error::SerializationError(_) => StatusCode::BAD_REQUEST,
            Error::IdAlreadyRegistered(_) => StatusCode::CONFLICT,
            Error::IdNotFound(_) => StatusCode::NOT_FOUND,
        };
        (status, ErrorResponseBody { message })
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, payload) = self.into();
        (status, Json(payload)).into_response()
    }
}
