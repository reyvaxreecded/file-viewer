use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use glassftp_shared::ApiError;
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug)]
pub struct Error(pub StatusCode, pub &'static str, pub String);
impl Error {
    pub fn bad(message: &str) -> Self {
        Self(StatusCode::BAD_REQUEST, "invalid_request", message.into())
    }
    pub fn conflict(message: &str) -> Self {
        Self(StatusCode::CONFLICT, "conflict", message.into())
    }
    pub fn internal() -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Erreur interne ; consulter les journaux du service".into(),
        )
    }
    pub fn not_found() -> Self {
        Self(
            StatusCode::NOT_FOUND,
            "not_found",
            "Élément introuvable".into(),
        )
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(ApiError {
                code: self.1.into(),
                message: self.2,
            }),
        )
            .into_response()
    }
}
impl From<glassftp_sftp::Error> for Error {
    fn from(e: glassftp_sftp::Error) -> Self {
        use glassftp_sftp::Error as E;
        let (s, c) = match e {
            E::NotFound => (404, "not_found"),
            E::Forbidden => (403, "forbidden"),
            E::Conflict => (409, "conflict"),
            E::Limit => (413, "limit"),
            E::Unsupported => (422, "unsupported"),
            _ => (503, "sftp_unavailable"),
        };
        Self(
            StatusCode::from_u16(s).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            c,
            e.to_string(),
        )
    }
}
impl From<glassftp_shared::InvalidPath> for Error {
    fn from(e: glassftp_shared::InvalidPath) -> Self {
        Self::bad(&e.to_string())
    }
}
impl From<glassftp_sftp::RawError> for Error {
    fn from(e: glassftp_sftp::RawError) -> Self {
        glassftp_sftp::Error::from(e).into()
    }
}
