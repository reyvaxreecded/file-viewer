use serde::{Deserialize, Serialize};

/// Validated virtual path, independent of the remote filesystem prefix.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct VPath(String);
#[derive(Debug, thiserror::Error)]
#[error("Chemin invalide ou réservé")]
pub struct InvalidPath;
impl VPath {
    pub fn new(value: &str) -> Result<Self, InvalidPath> {
        if value.len() > 4096
            || !value.starts_with('/')
            || value.contains('\\')
            || value.chars().any(char::is_control)
        {
            return Err(InvalidPath);
        }
        if value == "/" {
            return Ok(Self(value.into()));
        }
        for part in value[1..].split('/') {
            if part.is_empty()
                || part == "."
                || part == ".."
                || part.len() > 255
                || part.to_lowercase().starts_with(".glassftp")
            {
                return Err(InvalidPath);
            }
        }
        Ok(Self(value.into()))
    }
    pub fn root() -> Self {
        Self("/".into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn is_root(&self) -> bool {
        self.0 == "/"
    }
    pub fn name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or("")
    }
    pub fn parent(&self) -> Self {
        let p = self.0.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        Self(if p.is_empty() { "/" } else { p }.into())
    }
    pub fn join(&self, name: &str) -> Result<Self, InvalidPath> {
        if name.contains('/') {
            return Err(InvalidPath);
        }
        Self::new(&format!("{}/{}", self.0.trim_end_matches('/'), name))
    }
}
impl TryFrom<String> for VPath {
    type Error = InvalidPath;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(&s)
    }
}
impl From<VPath> for String {
    fn from(p: VPath) -> Self {
        p.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Directory,
    File,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub path: VPath,
    pub name: String,
    pub kind: FileKind,
    pub size: u64,
    pub modified: u64,
    pub mime: String,
    pub version: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Listing {
    pub path: VPath,
    pub entries: Vec<Entry>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PathRequest {
    pub path: VPath,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MoveRequest {
    pub source: VPath,
    pub destination: VPath,
    pub version: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VersionRequest {
    pub path: VPath,
    pub version: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UploadRequest {
    pub path: VPath,
    pub size: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Upload {
    pub id: String,
    pub path: VPath,
    pub size: u64,
    pub offset: u64,
    pub status: String,
    pub created: u64,
    #[serde(default)]
    pub updated: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrashItem {
    pub id: String,
    pub original: VPath,
    pub deleted: u64,
    pub entry: Entry,
    pub status: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RestoreRequest {
    pub destination: VPath,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextFile {
    pub path: VPath,
    pub content: String,
    pub version: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SaveText {
    pub path: VPath,
    pub content: String,
    pub version: String,
}
pub fn human_size(n: u64) -> String {
    if n < 1024 {
        format!("{n} o")
    } else if n < 1048576 {
        format!("{:.1} Ko", n as f64 / 1024.)
    } else if n < 1073741824 {
        format!("{:.1} Mo", n as f64 / 1048576.)
    } else {
        format!("{:.2} Go", n as f64 / 1073741824.)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsafe_paths() {
        for s in [
            "",
            "../a",
            "/../a",
            "/a/../b",
            "/a//b",
            "/a/",
            "/a/./b",
            "/a\\b",
            "/a\0",
            "/.glassftp/x",
            "/a/.GLASSFTP-trash",
        ] {
            assert!(VPath::new(s).is_err(), "{s:?}");
        }
    }
    #[test]
    fn preserves_names_without_double_decoding() {
        for s in ["/", "/équipe/été 2026.png", "/100%.txt", "/%2e%2e"] {
            assert_eq!(VPath::new(s).unwrap().as_str(), s);
        }
        assert_eq!(
            VPath::root().join("photos").unwrap().parent(),
            VPath::root()
        );
        assert!(VPath::root().join("../foo").is_err());
    }
    #[test]
    fn validates_deserialization() {
        assert!(serde_json::from_str::<PathRequest>(r#"{"path":"/../secret"}"#).is_err());
        let p = VPath::new("/docs/a.txt").unwrap();
        assert_eq!(
            serde_json::from_str::<VPath>(&serde_json::to_string(&p).unwrap()).unwrap(),
            p
        );
    }
}
