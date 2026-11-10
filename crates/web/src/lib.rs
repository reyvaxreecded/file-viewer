mod api;
mod state;
mod views;
use glassftp_shared::*;
use leptos::prelude::*;
use state::State;
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::spawn_local;

#[wasm_bindgen(raw_module = "/bridge.js")]
extern "C" {
    fn install();
    #[wasm_bindgen(js_name = pickFiles)]
    fn pick_files(path: &str, folder: bool);
    #[wasm_bindgen(js_name = setDirty)]
    fn set_dirty(dirty: bool);
}
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
    install();
}
#[component]
fn App() -> impl IntoView {
    let s = State::new();
    let previous_location = RwSignal::new(String::new());
    Effect::new(move |_| {
        let path = s.path.get();
        let page = s.page.get();
        let mode = s.mode.get();
        let _ = s.refresh.get();
        s.generation.update(|g| *g += 1);
        let gen_id = s.generation.get_untracked();
        let location = format!("{mode}:{}:{page}", path.as_str());
        let navigating = location != previous_location.get_untracked();
        previous_location.set(location);
        s.loading.set(navigating);
        s.error.set(String::new());
        if navigating {
            s.selected.set(vec![]);
        }
        spawn_local(async move {
            let result = async {
                if mode == "trash" {
                    let items = api::get::<Vec<TrashItem>>("/api/v1/trash").await?;
                    if s.generation.get_untracked() == gen_id {
                        s.trash.set(items);
                    }
                    return Ok::<_, String>(());
                }
                if mode == "favorites" || mode == "recent" {
                    let paths = if mode == "favorites" {
                        s.favorites.get_untracked()
                    } else {
                        s.recent.get_untracked()
                    };
                    let mut entries = vec![];
                    for p in paths.into_iter().take(100) {
                        if let Ok(e) = api::get::<Entry>(&format!(
                            "/api/v1/files/info?path={}",
                            api::encode(&p)
                        ))
                        .await
                        {
                            entries.push(e);
                        }
                    }
                    if s.generation.get_untracked() == gen_id {
                        s.total.set(entries.len());
                        s.apply_entries(entries);
                    }
                    return Ok(());
                }
                let listing = api::get::<Listing>(&format!(
                    "/api/v1/files?path={}&offset={}&limit=200",
                    api::encode(path.as_str()),
                    page * 200
                ))
                .await?;
                if s.generation.get_untracked() == gen_id {
                    s.total.set(listing.total);
                    s.apply_entries(listing.entries);
                }
                Ok(())
            }
            .await;
            if s.generation.get_untracked() == gen_id {
                if let Err(e) = result {
                    s.error.set(e);
                    s.entries.set(vec![]);
                    s.selected.set(vec![]);
                }
                s.loading.set(false);
            }
        });
    });
    Effect::new(move |_| {
        let draft = s.draft.get();
        let original = s.text.get();
        set_dirty(original.is_some_and(|t| t.content != draft));
    });
    let handler =
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
            let command = e.ctrl_key() || e.meta_key();
            let target = e
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok());
            let typing = target.is_some_and(|el| {
                matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                    || el.get_attribute("contenteditable").is_some()
            });
            if command && e.key().to_lowercase() == "s" && s.editing.get_untracked() {
                e.prevent_default();
                s.save();
                return;
            }
            if e.key() == "Escape" {
                s.menu.set(None);
                s.dialog.set(None);
                if s.preview.get_untracked().is_some() {
                    s.close_preview();
                } else {
                    s.selected.set(vec![]);
                }
                return;
            }
            if typing || s.dialog.get_untracked().is_some() || s.preview.get_untracked().is_some() {
                return;
            }
            match e.key().as_str() {
                "a" | "A" if command => {
                    e.prevent_default();
                    s.selected.set(
                        s.visible()
                            .iter()
                            .map(|e| e.path.as_str().to_string())
                            .collect(),
                    );
                }
                "c" | "C" if command => {
                    e.prevent_default();
                    s.clipboard.set(Some((s.chosen(), false)));
                    s.notice.set("Sélection copiée".into());
                }
                "x" | "X" if command => {
                    e.prevent_default();
                    s.clipboard.set(Some((s.chosen(), true)));
                    s.notice.set("Sélection coupée".into());
                }
                "v" | "V" if command => {
                    e.prevent_default();
                    s.paste(s.path.get_untracked());
                }
                "f" | "F" if command => {
                    e.prevent_default();
                    if let Some(el) = web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|d| d.get_element_by_id("search"))
                        .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
                    {
                        let _ = el.focus();
                    }
                }
                "F2" => {
                    e.prevent_default();
                    if s.chosen().len() == 1 {
                        s.ask("rename");
                    }
                }
                "Delete" => {
                    e.prevent_default();
                    if !s.chosen().is_empty() {
                        s.ask("trash");
                    }
                }
                "Enter" => {
                    e.prevent_default();
                    if let Some(e) = s.chosen().first() {
                        s.open(e.clone());
                    }
                }
                "ArrowLeft" if e.alt_key() => {
                    e.prevent_default();
                    s.back(false);
                }
                "ArrowRight" if e.alt_key() => {
                    e.prevent_default();
                    s.back(true);
                }
                _ => (),
            }
        });
    if let Some(w) = web_sys::window() {
        let _ = w.add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
    }
    handler.forget();
    let change = Closure::<dyn FnMut(web_sys::CustomEvent)>::new(move |e: web_sys::CustomEvent| {
        let path = e.detail().as_string().unwrap_or_default();
        if path == "*"
            || path == s.path.get_untracked().as_str()
            || s.mode.get_untracked() != "files"
        {
            s.reload();
        }
    });
    if let Some(w) = web_sys::window() {
        let _ =
            w.add_event_listener_with_callback("glassftp-change", change.as_ref().unchecked_ref());
    }
    change.forget();
    let internal =
        Closure::<dyn FnMut(web_sys::CustomEvent)>::new(move |e: web_sys::CustomEvent| {
            if let Some(p) = e.detail().as_string().and_then(|p| VPath::new(&p).ok()) {
                s.clipboard.set(Some((s.chosen(), true)));
                s.paste(p);
            }
        });
    if let Some(w) = web_sys::window() {
        let _ =
            w.add_event_listener_with_callback("glassftp-move", internal.as_ref().unchecked_ref());
    }
    internal.forget();
    view! {
        <div class="ambient ambient-one"></div><div class="ambient ambient-two"></div>
        <div id="glassftp" class="app-shell" class:sidebar-hidden=move || !s.sidebar.get() data-path=move || s.path.get().as_str().to_owned()>
            <views::Sidebar state=s />
            <main class="workspace">
                <views::Toolbar state=s />
                <div class="workspace-body">
                    <section class="explorer" id="dropzone" aria-label="Explorateur de fichiers">
                        <div class="section-title"><div><div class="eyebrow">"VOTRE ESPACE PARTAGÉ"</div><h1>{move ||match s.mode.get().as_str(){"favorites"=>"Favoris".to_string(),"recent"=>"Récents".into(),"trash"=>"Corbeille".into(),_=>if s.path.get().is_root(){"Tous les fichiers".into()}else{s.path.get().name().to_owned()}}}</h1><p>{move ||if s.mode.get()=="trash"{"Les fichiers supprimés restent ici jusqu’à leur suppression définitive.".into()}else{format!("{} éléments · Un même espace pour toute l’équipe",s.total.get())}}</p></div>
                        <div class="view-toggle"><button title="Vue grille" class:active=move ||s.grid.get() on:click=move |_|s.grid.set(true)><views::Icon name="grid"/></button><button title="Vue liste" class:active=move ||!s.grid.get() on:click=move |_|s.grid.set(false)><views::Icon name="list"/></button></div></div>
                        <Show when=move ||!s.error.get().is_empty()><div class="error-banner" role="alert"><span>{move ||s.error.get()}</span><button on:click=move |_|s.reload()>"Réessayer"</button><button title="Fermer l’erreur" on:click=move |_|s.error.set(String::new())>"×"</button></div></Show>
                        <Show when=move ||s.loading.get()><div class="skeletons" aria-label="Chargement">{(0..8).map(|_|view!{<div class="skeleton"></div>}).collect_view()}</div></Show>
                        <Show when=move ||!s.loading.get()>
                            <Show when=move ||s.mode.get()=="trash" fallback=move ||view!{<views::Files state=s/>}><views::Trash state=s/></Show>
                        </Show>
                        <div class="drop-overlay"><div class="drop-symbol">"↓"</div><h2>"Déposez vos fichiers ici"</h2><p>"Ils seront partagés avec toute l’équipe"</p></div>
                        <div class="pagination"><span>{move ||format!("{} sélectionné(s)",s.selected.get().len())}</span><div><button disabled=move ||s.page.get()==0 on:click=move |_|s.page.update(|p|*p=p.saturating_sub(1))>"←"</button><span>{move ||format!("Page {}",s.page.get()+1)}</span><button disabled=move || { (s.page.get()+1)*200 >= s.total.get() } on:click=move |_|s.page.update(|p|*p+=1)>"→"</button></div></div>
                    </section>
                    <Show when=move ||s.details.get()><views::Inspector state=s/></Show>
                </div>
                <footer class="statusbar"><span class="connection"><i></i><span id="connection-label">"Connexion…"</span></span><span>"Espace d’équipe · SFTP"</span><button on:click=move |_|s.details.update(|v|*v=!*v)>"Détails"</button></footer>
            </main>
        </div>
        <aside id="transfer-panel" class="transfer-panel" hidden><div class="transfer-heading"><h3>"Transferts"</h3><button id="minimize-transfers" title="Réduire les transferts">"−"</button></div><div id="transfer-list"></div></aside>
        <Show when=move ||!s.notice.get().is_empty()><div class="toast" role="status"><span>"✓ "{move ||s.notice.get()}</span><button title="Fermer la notification" on:click=move |_|s.notice.set(String::new())>"×"</button></div></Show>
        <views::Dialogs state=s/><views::Preview state=s/><views::ContextMenu state=s/>
    }
}
