use crate::api;
use glassftp_shared::*;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
#[derive(Clone, Copy)]
pub struct State {
    pub path: RwSignal<VPath>,
    pub entries: RwSignal<Vec<Entry>>,
    pub selected: RwSignal<Vec<String>>,
    pub loading: RwSignal<bool>,
    pub error: RwSignal<String>,
    pub notice: RwSignal<String>,
    pub mode: RwSignal<String>,
    pub grid: RwSignal<bool>,
    pub details: RwSignal<bool>,
    pub sidebar: RwSignal<bool>,
    pub search: RwSignal<String>,
    pub sort: RwSignal<String>,
    pub total: RwSignal<usize>,
    pub page: RwSignal<usize>,
    pub favorites: RwSignal<Vec<String>>,
    pub recent: RwSignal<Vec<String>>,
    pub trash: RwSignal<Vec<TrashItem>>,
    pub history: RwSignal<Vec<VPath>>,
    pub position: RwSignal<usize>,
    pub refresh: RwSignal<u64>,
    pub generation: RwSignal<u64>,
    pub dialog: RwSignal<Option<String>>,
    pub input: RwSignal<String>,
    pub preview: RwSignal<Option<Entry>>,
    pub text: RwSignal<Option<TextFile>>,
    pub draft: RwSignal<String>,
    pub editing: RwSignal<bool>,
    pub saving: RwSignal<bool>,
    pub menu: RwSignal<Option<(i32, i32)>>,
    pub clipboard: RwSignal<Option<(Vec<Entry>, bool)>>,
}
pub fn load_local(key: &str) -> Vec<String> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(key).ok().flatten())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
pub fn persist(key: &str, items: &[String]) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item(key, &serde_json::to_string(items).unwrap_or_default());
    }
}
impl State {
    pub fn apply_entries(self, entries: Vec<Entry>) {
        self.selected
            .update(|ids| ids.retain(|id| entries.iter().any(|e| e.path.as_str() == id)));
        self.entries.set(entries);
    }
    pub fn new() -> Self {
        Self {
            path: RwSignal::new(VPath::root()),
            entries: RwSignal::new(vec![]),
            selected: RwSignal::new(vec![]),
            loading: RwSignal::new(true),
            error: RwSignal::new(String::new()),
            notice: RwSignal::new(String::new()),
            mode: RwSignal::new("files".into()),
            grid: RwSignal::new(true),
            details: RwSignal::new(true),
            sidebar: RwSignal::new(true),
            search: RwSignal::new(String::new()),
            sort: RwSignal::new("name".into()),
            total: RwSignal::new(0),
            page: RwSignal::new(0),
            favorites: RwSignal::new(load_local("glassftp.favorites")),
            recent: RwSignal::new(load_local("glassftp.recent")),
            trash: RwSignal::new(vec![]),
            history: RwSignal::new(vec![VPath::root()]),
            position: RwSignal::new(0),
            refresh: RwSignal::new(0),
            generation: RwSignal::new(0),
            dialog: RwSignal::new(None),
            input: RwSignal::new(String::new()),
            preview: RwSignal::new(None),
            text: RwSignal::new(None),
            draft: RwSignal::new(String::new()),
            editing: RwSignal::new(false),
            saving: RwSignal::new(false),
            menu: RwSignal::new(None),
            clipboard: RwSignal::new(None),
        }
    }
    pub fn reload(self) {
        self.refresh.update(|n| *n += 1);
    }
    pub fn navigate(self, p: VPath) {
        self.mode.set("files".into());
        self.search.set(String::new());
        self.page.set(0);
        let pos = self.position.get_untracked();
        self.history.update(|h| {
            h.truncate(pos + 1);
            h.push(p.clone());
        });
        self.position.set(pos + 1);
        self.path.set(p);
        self.selected.set(vec![]);
    }
    pub fn back(self, forward: bool) {
        let p = self.position.get_untracked();
        let h = self.history.get_untracked();
        let n = if forward { p + 1 } else { p.saturating_sub(1) };
        if n < h.len() {
            self.position.set(n);
            self.path.set(h[n].clone());
            self.mode.set("files".into());
            self.page.set(0);
            self.selected.set(vec![]);
        }
    }
    pub fn chosen(self) -> Vec<Entry> {
        let ids = self.selected.get_untracked();
        self.entries
            .get_untracked()
            .into_iter()
            .filter(|e| ids.contains(&e.path.as_str().to_string()))
            .collect()
    }
    pub fn visible(self) -> Vec<Entry> {
        let mut e = self.entries.get();
        let query = self.search.get().to_lowercase();
        e.retain(|e| e.name.to_lowercase().contains(&query));
        let sort = self.sort.get();
        e.sort_by(|a, b| {
            let folders = (a.kind != FileKind::Directory).cmp(&(b.kind != FileKind::Directory));
            folders.then_with(|| match sort.as_str() {
                "size" => b.size.cmp(&a.size),
                "modified" => b.modified.cmp(&a.modified),
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            })
        });
        e
    }
    pub fn select(self, e: &Entry, multi: bool, range: bool) {
        let id = e.path.as_str().to_string();
        if range {
            let all = self.visible();
            let current = self.selected.get_untracked();
            if let (Some(start), Some(end)) = (
                current
                    .first()
                    .and_then(|id| all.iter().position(|e| e.path.as_str() == id)),
                all.iter().position(|e| e.path.as_str() == id),
            ) {
                self.selected.set(
                    all[start.min(end)..=start.max(end)]
                        .iter()
                        .map(|e| e.path.as_str().to_string())
                        .collect(),
                );
                return;
            }
        }
        if multi {
            self.selected.update(|s| {
                if s.contains(&id) {
                    s.retain(|p| p != &id)
                } else {
                    s.push(id.clone())
                }
            });
        } else {
            self.selected.set(vec![id]);
        }
    }
    pub fn open(self, e: Entry) {
        if e.kind == FileKind::Directory {
            self.navigate(e.path);
            return;
        }
        self.recent.update(|r| {
            r.retain(|p| p != e.path.as_str());
            r.insert(0, e.path.as_str().into());
            r.truncate(50);
        });
        persist("glassftp.recent", &self.recent.get_untracked());
        self.preview.set(Some(e.clone()));
        self.text.set(None);
        self.editing.set(false);
        self.draft.set(String::new());
        if is_text(&e) {
            spawn_local(async move {
                match api::get::<TextFile>(&format!(
                    "/api/v1/text?path={}",
                    api::encode(e.path.as_str())
                ))
                .await
                {
                    Ok(t) => {
                        if self
                            .preview
                            .get_untracked()
                            .is_some_and(|p| p.path == t.path)
                        {
                            self.draft.set(t.content.clone());
                            self.text.set(Some(t));
                        }
                    }
                    Err(e) => self.error.set(e),
                }
            });
        }
    }
    pub fn favorite(self, e: Entry) {
        let p = e.path.as_str().to_string();
        self.favorites.update(|f| {
            if f.contains(&p) {
                f.retain(|x| x != &p)
            } else {
                f.push(p)
            }
        });
        persist("glassftp.favorites", &self.favorites.get_untracked());
    }
    pub fn ask(self, action: &str) {
        self.menu.set(None);
        self.input.set(if action == "rename" {
            self.chosen()
                .first()
                .map(|e| e.name.clone())
                .unwrap_or_default()
        } else {
            String::new()
        });
        self.dialog.set(Some(action.into()));
    }
    pub fn dirty(self) -> bool {
        self.text
            .get_untracked()
            .is_some_and(|t| t.content != self.draft.get_untracked())
    }
    pub fn close_preview(self) {
        if self.dirty()
            && !web_sys::window().is_some_and(|w| {
                w.confirm_with_message("Abandonner les modifications non enregistrées ?")
                    .unwrap_or(false)
            })
        {
            return;
        }
        self.preview.set(None);
        self.text.set(None);
        self.draft.set(String::new());
        self.editing.set(false);
    }
    pub fn save(self) {
        if self.saving.get_untracked() {
            return;
        }
        let Some(t) = self.text.get_untracked() else {
            return;
        };
        self.saving.set(true);
        spawn_local(async move {
            let q = SaveText {
                path: t.path,
                version: t.version,
                content: self.draft.get_untracked(),
            };
            match api::send("PUT", "/api/v1/text", &q).await {
                Ok(body) => match serde_json::from_str::<TextFile>(&body) {
                    Ok(t) => {
                        self.text.set(Some(t));
                        self.notice.set("Modifications enregistrées".into());
                        self.reload();
                    }
                    Err(_) => self.error.set("Réponse de sauvegarde invalide".into()),
                },
                Err(e) => self.error.set(e),
            };
            self.saving.set(false);
        });
    }
    pub fn perform(self) {
        let action = self.dialog.get_untracked().unwrap_or_default();
        let input = self.input.get_untracked();
        let selected = self.chosen();
        spawn_local(async move {
            let result: Result<(), String> = async {
                match action.as_str() {
                    "folder" => {
                        let p = self
                            .path
                            .get_untracked()
                            .join(input.trim())
                            .map_err(|e| e.to_string())?;
                        api::send("POST", "/api/v1/folders", &PathRequest { path: p }).await?;
                    }
                    "rename" => {
                        let e = selected.first().ok_or("Sélectionnez un élément")?;
                        let to = e
                            .path
                            .parent()
                            .join(input.trim())
                            .map_err(|e| e.to_string())?;
                        api::send(
                            "PATCH",
                            "/api/v1/files/rename",
                            &MoveRequest {
                                source: e.path.clone(),
                                destination: to,
                                version: e.version.clone(),
                            },
                        )
                        .await?;
                    }
                    "trash" => {
                        for e in selected {
                            api::send(
                                "POST",
                                "/api/v1/files/trash",
                                &VersionRequest {
                                    path: e.path,
                                    version: e.version,
                                },
                            )
                            .await?;
                        }
                    }
                    _ => return Err("Action inconnue".into()),
                }
                Ok(())
            }
            .await;
            match result {
                Ok(()) => {
                    self.dialog.set(None);
                    self.selected.set(vec![]);
                    self.notice.set("Opération effectuée".into());
                }
                Err(e) => self.error.set(e),
            };
            self.reload();
        });
    }
    pub fn paste(self, target: VPath) {
        let Some((entries, cut)) = self.clipboard.get_untracked() else {
            return;
        };
        spawn_local(async move {
            for e in entries {
                let dest = match target.join(&e.name) {
                    Ok(p) => p,
                    Err(e) => {
                        self.error.set(e.to_string());
                        return;
                    }
                };
                match api::send(
                    "POST",
                    if cut {
                        "/api/v1/files/move"
                    } else {
                        "/api/v1/files/copy"
                    },
                    &MoveRequest {
                        source: e.path,
                        destination: dest,
                        version: e.version,
                    },
                )
                .await
                {
                    Ok(_) => (),
                    Err(e) => {
                        self.error.set(e);
                        self.reload();
                        return;
                    }
                }
            }
            if cut {
                self.clipboard.set(None);
            }
            self.notice.set("Éléments transférés".into());
            self.reload();
        });
    }
}
pub fn is_text(e: &Entry) -> bool {
    let ext = e.name.rsplit('.').next().unwrap_or("").to_lowercase();
    e.mime.starts_with("text/")
        || matches!(
            ext.as_str(),
            "rs" | "py"
                | "js"
                | "ts"
                | "tsx"
                | "jsx"
                | "json"
                | "md"
                | "toml"
                | "yaml"
                | "yml"
                | "sql"
                | "sh"
                | "conf"
                | "ini"
                | "xml"
                | "csv"
                | "log"
                | "env"
        )
}
