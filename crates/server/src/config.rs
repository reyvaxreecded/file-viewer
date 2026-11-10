use std::{
    env,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    time::Duration,
};
pub struct Config {
    pub listen: SocketAddr,
    pub origin: String,
    pub authority: String,
    pub assets: PathBuf,
    pub database: PathBuf,
    pub max_upload: u64,
    pub transfers: usize,
    pub sftp: glassftp_sftp::Config,
}
fn value(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.into())
}
fn required(key: &str) -> Result<String, String> {
    env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("Configuration manquante : {key}"))
}
fn number<T: std::str::FromStr>(key: &str, default: &str) -> Result<T, String> {
    value(key, default)
        .parse()
        .map_err(|_| format!("Valeur invalide : {key}"))
}
impl Config {
    pub fn load() -> Result<Self, String> {
        let host: IpAddr = number("APP_HOST", "127.0.0.1")?;
        if !host.is_loopback() {
            return Err("APP_HOST doit être une adresse loopback".into());
        }
        let port = number("APP_PORT", "8080")?;
        let base =
            url::Url::parse(&required("APP_BASE_URL")?).map_err(|_| "APP_BASE_URL invalide")?;
        if base.path() != "/"
            || base.query().is_some()
            || base.fragment().is_some()
            || !base.username().is_empty()
            || base.password().is_some()
        {
            return Err("APP_BASE_URL doit être une origine sans chemin ni identifiants".into());
        }
        if base.scheme() != "https"
            && !(base.scheme() == "http"
                && matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
        {
            return Err("HTTPS requis hors développement loopback".into());
        }
        let origin = base.origin().ascii_serialization();
        let authority = origin
            .split_once("://")
            .ok_or("Origine invalide")?
            .1
            .to_owned();
        let password = if let Ok(path) = env::var("SFTP_PASSWORD_FILE") {
            std::fs::read_to_string(path)
                .map_err(|_| "Lecture de SFTP_PASSWORD_FILE impossible")?
                .trim_end_matches(['\r', '\n'])
                .to_owned()
        } else {
            required("SFTP_PASSWORD")?
        };
        if password.is_empty() {
            return Err("Mot de passe SFTP vide".into());
        }
        let fingerprint = required("SFTP_HOST_KEY_SHA256")?;
        if !fingerprint.starts_with("SHA256:") || fingerprint.len() < 40 {
            return Err("Empreinte SHA256 de clé hôte obligatoire".into());
        }
        let root = required("SFTP_ROOT")?;
        if !root.starts_with('/')
            || root.split('/').any(|p| p == ".." || p == ".")
            || root.contains('\\')
        {
            return Err("SFTP_ROOT doit être un chemin absolu canonique".into());
        }
        let transfers: usize = number("MAX_CONCURRENT_TRANSFERS", "4")?;
        let connections: usize = number("SFTP_MAX_CONNECTIONS", "6")?;
        if !(1..=16).contains(&transfers) || connections < transfers + 1 || connections > 32 {
            return Err(
                "Prévoir 1 à 16 transferts et au moins une connexion supplémentaire (max 32)"
                    .into(),
            );
        }
        let timeout: u64 = number("SFTP_TIMEOUT_SECONDS", "30")?;
        if !(5..=300).contains(&timeout) {
            return Err("SFTP_TIMEOUT_SECONDS doit être entre 5 et 300".into());
        }
        Ok(Self {
            listen: SocketAddr::new(host, port),
            origin,
            authority,
            assets: value("ASSET_DIRECTORY", "dist").into(),
            database: value("DATABASE_PATH", "glassftp.sqlite").into(),
            max_upload: number("MAX_UPLOAD_SIZE", "107374182400")?,
            transfers,
            sftp: glassftp_sftp::Config {
                host: value("SFTP_HOST", "127.0.0.1"),
                port: number("SFTP_PORT", "22")?,
                username: required("SFTP_USERNAME")?,
                password,
                root,
                host_key_sha256: fingerprint,
                connections,
                timeout: Duration::from_secs(timeout),
            },
        })
    }
}
