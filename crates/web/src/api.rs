use gloo_net::http::Request;
use serde::{Serialize, de::DeserializeOwned};
pub fn encode(s: &str) -> String {
    js_sys::encode_uri_component(s)
        .as_string()
        .unwrap_or_default()
}
pub async fn get<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    let response = Request::get(path)
        .send()
        .await
        .map_err(|_| "Connexion au serveur interrompue".to_owned())?;
    if !response.ok() {
        return Err(message(response).await);
    }
    response
        .json()
        .await
        .map_err(|_| "Réponse serveur invalide".into())
}
pub async fn send<T: Serialize>(method: &str, path: &str, data: &T) -> Result<String, String> {
    let builder = match method {
        "PATCH" => Request::patch(path),
        "PUT" => Request::put(path),
        "DELETE" => Request::delete(path),
        _ => Request::post(path),
    };
    let response = builder
        .header("X-GlassFTP", "1")
        .json(data)
        .map_err(|_| "Requête invalide")?
        .send()
        .await
        .map_err(|_| "Connexion interrompue".to_owned())?;
    if !response.ok() {
        return Err(message(response).await);
    }
    response
        .text()
        .await
        .map_err(|_| "Réponse interrompue".into())
}
async fn message(response: gloo_net::http::Response) -> String {
    let status = response.status();
    response
        .json::<glassftp_shared::ApiError>()
        .await
        .map(|e| e.message)
        .unwrap_or_else(|_| format!("Requête refusée ({status})"))
}
