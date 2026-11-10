use crate::{App, error::Error};
use axum::{
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
pub async fn guard(State(app): State<App>, req: Request, next: Next) -> Response {
    let h = req.headers();
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok());
    if host != Some(app.config.authority.as_str()) {
        return Error(
            StatusCode::FORBIDDEN,
            "host",
            "Hôte HTTP non autorisé".into(),
        )
        .into_response();
    }
    let origin = h.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let write = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if origin.is_some_and(|s| s != app.config.origin)
        || h.get("sec-fetch-site").is_some_and(|s| s == "cross-site")
        || (write
            && (origin != Some(app.config.origin.as_str())
                || h.get("x-glassftp").and_then(|v| v.to_str().ok()) != Some("1")))
    {
        return Error(
            StatusCode::FORBIDDEN,
            "origin",
            "Origine de la requête non autorisée".into(),
        )
        .into_response();
    }
    let mut response = next.run(req).await;
    let h = response.headers_mut();
    h.entry(header::CONTENT_SECURITY_POLICY).or_insert(HeaderValue::from_static("default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' blob:; media-src 'self' blob:; connect-src 'self'; object-src 'none'; frame-src 'self' blob:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
