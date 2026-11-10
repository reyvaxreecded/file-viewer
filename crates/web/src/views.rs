use crate::{
    api, pick_files,
    state::{State, is_text},
};
use glassftp_shared::*;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn Icon(name: &'static str) -> impl IntoView {
    let d = match name {
        "grid" => "M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z",
        "list" => "M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01",
        "folder" => "M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z",
        "star" => "m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9Z",
        "clock" => "M12 8v5l3 2M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0",
        "trash" => "M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7",
        "upload" => "M12 16V3m-5 5 5-5 5 5M4 15v6h16v-6",
        "download" => "M12 3v13m-5-5 5 5 5-5M4 16v5h16v-5",
        "search" => "m21 21-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0",
        "refresh" => "M20 7v5h-5M4 17v-5h5M5 8a8 8 0 0 1 13-3l2 3M4 16l2 3a8 8 0 0 0 13-3",
        "home" => "m3 10 9-7 9 7v11h-7v-8h-4v8H3Z",
        "sun" => {
            "M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1 1M18 18l1 1M5 19l1-1M18 6l1-1M17 12a5 5 0 1 1-10 0 5 5 0 0 1 10 0"
        }
        "file" => "M5 3h9l5 5v13H5ZM14 3v6h5M8 13h8M8 17h6",
        "more" => "M5 12h.01M12 12h.01M19 12h.01",
        _ => "M12 4v16M4 12h16",
    };
    view! {<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d=d/></svg>}
}
#[component]
pub fn Sidebar(state: State) -> impl IntoView {
    let s = state;
    view! {<aside class="sidebar"><a class="brand" href="/" aria-label="GlassFTP accueil"><span class="brand-mark">"G"</span><span>"glass"<b>"ftp"</b><small>"L’ESPACE DE VOTRE ÉQUIPE"</small></span></a>
        <div class="space-badge"><span class="space-avatar">"E"</span><div><strong>"Espace partagé"</strong><small>"Réseau local"</small></div><span class="badge-chevron">"⌄"</span></div>
        <span class="nav-label">"EXPLORER"</span><nav>
        <button on:click=move |_|s.navigate(VPath::root())><Icon name="home"/><span>"Accueil"</span></button>
        {[("files","Tous les fichiers","folder"),("recent","Récents","clock"),("favorites","Favoris","star"),("trash","Corbeille","trash")].into_iter().map(move |(mode,label,icon)|view!{
            <button class:active=move ||s.mode.get()==mode on:click=move |_|{s.mode.set(mode.into());s.page.set(0);s.search.set(String::new());s.reload();}><Icon name=icon/><span>{label}</span>{if mode=="files"{Some(view!{<span class="nav-dot"></span>})}else{None}}</button>
        }).collect_view()}</nav>
        <div class="sidebar-section"><span class="nav-label">"ÉPINGLÉS"</span><button title="Épingler le dossier courant" on:click=move |_|{let p=s.path.get_untracked();if !p.is_root(){spawn_local(async move{if let Ok(e)=api::get::<Entry>(&format!("/api/v1/files/info?path={}",api::encode(p.as_str()))).await{s.favorite(e);}});}}><Icon name="plus"/></button></div>
        <div class="pinned">{move ||s.favorites.get().into_iter().take(6).map(move |path|{let label=path.rsplit('/').next().unwrap_or("").to_string();view!{<button on:click=move |_|{let path=path.clone();spawn_local(async move{match api::get::<Entry>(&format!("/api/v1/files/info?path={}",api::encode(&path))).await{Ok(e)=>s.open(e),Err(e)=>s.error.set(e)}});} ><span class="pin-dot"></span><span>{label}</span></button>}}).collect_view()}</div>
        <div class="sidebar-bottom"><div class="local-note"><span class="local-orbit">"◎"</span><strong>"Vos fichiers. Chez vous."</strong><p>"Un espace local, partagé et toujours à portée de main."</p></div><button class="theme-button" on:click=move |_|{if let Some(d)=web_sys::window().and_then(|w|w.document()).and_then(|d|d.document_element()){let light=d.get_attribute("data-theme").as_deref()==Some("light");let theme=if light{"dark"}else{"light"};let _=d.set_attribute("data-theme",theme);if let Some(st)=web_sys::window().and_then(|w|w.local_storage().ok().flatten()){let _=st.set_item("glassftp.theme",theme);}}}><Icon name="sun"/><span>"Changer d’apparence"</span></button><div class="sidebar-version">"GLASSFTP"<span>"v0.1"</span></div></div>
    </aside>}
}
#[component]
pub fn Toolbar(state: State) -> impl IntoView {
    let s = state;
    view! {<header class="toolbar"><div class="navigation"><button title="Réduire la barre latérale" on:click=move |_|s.sidebar.update(|v|*v=!*v)><Icon name="list"/></button><button title="Précédent (Alt+←)" disabled=move ||s.position.get()==0 on:click=move |_|s.back(false)>"‹"</button><button title="Suivant (Alt+→)" disabled=move || { s.position.get()+1 >= s.history.get().len() } on:click=move |_|s.back(true)>"›"</button></div><div class="breadcrumbs"><button title="Racine du partage" aria-label="Racine du partage" on:click=move |_|s.navigate(VPath::root())><Icon name="home"/></button>{move ||{let p=s.path.get();let mut current=String::new();p.as_str().split('/').filter(|p|!p.is_empty()).map(|part|{current.push('/');current.push_str(part);let dest=current.clone();view!{<span>"/"</span><button on:click=move |_|if let Ok(p)=VPath::new(&dest){s.navigate(p);}>{part.to_string()}</button>}}).collect_view()}}</div>
        <div class="toolbar-actions"><div class="searchbox"><Icon name="search"/><input id="search" type="search" placeholder="Filtrer cette page…" aria-label="Filtrer les fichiers affichés" prop:value=move ||s.search.get() on:input=move |e|s.search.set(event_target_value(&e))/><kbd>"⌘ F"</kbd></div><button title="Actualiser" on:click=move |_|s.reload()><Icon name="refresh"/></button><button class="secondary" title="Créer un dossier" aria-label="Créer un dossier" on:click=move |_|s.ask("folder")><Icon name="plus"/><span>"Nouveau"</span></button><Show when=move ||s.clipboard.get().is_some()><button class="secondary" on:click=move |_|s.paste(s.path.get_untracked())>"Coller"</button></Show><button class="primary" on:click=move |_|pick_files(s.path.get_untracked().as_str(),false)><Icon name="upload"/><span>"Importer"</span></button></div>
    </header>}
}
pub fn date(t: u64) -> String {
    js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(t as f64 * 1000.))
        .to_locale_date_string("fr-FR", &wasm_bindgen::JsValue::UNDEFINED)
        .as_string()
        .unwrap_or_default()
}
pub fn download_url(p: &VPath, inline: bool) -> String {
    format!(
        "/api/v1/download?path={}&inline={inline}",
        api::encode(p.as_str())
    )
}
#[component]
pub fn FileArt(entry: Entry) -> impl IntoView {
    let e = entry;
    let ext = e.name.rsplit('.').next().unwrap_or("FILE").to_uppercase();
    if e.kind == FileKind::Directory {
        view! {<div class="folder-art"><span></span></div>}.into_any()
    } else {
        let class = if e.mime.starts_with("image/") {
            "file-art image-art"
        } else if e.mime == "application/pdf" {
            "file-art pdf-art"
        } else if is_text(&e) {
            "file-art code-art"
        } else {
            "file-art"
        };
        view!{<div class=class><Icon name="file"/><span>{ext.chars().take(5).collect::<String>()}</span></div>}.into_any()
    }
}
#[component]
pub fn Files(state: State) -> impl IntoView {
    let s = state;
    view! {<div class="file-tools"><span>{move ||if s.search.get().is_empty(){"Fichiers et dossiers".into()}else{format!("{} résultat(s) sur cette page",s.visible().len())}}</span><select aria-label="Trier les fichiers" on:change=move |e|s.sort.set(event_target_value(&e))><option value="name">"Nom · A à Z"</option><option value="modified">"Plus récents"</option><option value="size">"Plus volumineux"</option></select></div>
    <Show when=move ||!s.visible().is_empty() fallback=move ||view!{<div class="empty-state"><div class="empty-icon"><Icon name="folder"/></div><h2>{move ||if s.search.get().is_empty(){"Un peu d’espace pour vos idées"}else{"Aucun résultat"}}</h2><p>"Glissez vos fichiers ici ou importez-les depuis votre ordinateur."</p><button class="primary" on:click=move |_|pick_files(s.path.get_untracked().as_str(),false)>"Importer des fichiers"</button><button class="text-button" on:click=move |_|pick_files(s.path.get_untracked().as_str(),true)>"Importer un dossier"</button></div>}>
    <div class="file-list-head" class:hidden=move ||s.grid.get()><button on:click=move |_|s.sort.set("name".into())>"Nom"</button><span>"Type"</span><button on:click=move |_|s.sort.set("size".into())>"Taille"</button><button on:click=move |_|s.sort.set("modified".into())>"Modification"</button></div>
    <div class="files" class:list=move ||!s.grid.get()>{move ||s.visible().into_iter().map(move |e|view!{<FileCard entry=e state=s/>}).collect_view()}</div></Show>}
}
#[component]
fn FileCard(entry: Entry, state: State) -> impl IntoView {
    let s = state;
    let e = entry;
    let click = e.clone();
    let open = e.clone();
    let menu = e.clone();
    let fav = e.clone();
    let select_path = e.path.as_str().to_string();
    let star_path = select_path.clone();
    let folder = e.kind == FileKind::Directory;
    view! {<article class="file-card" class:selected=move ||s.selected.get().contains(&select_path) tabindex="0" role="button" aria-label=e.name.clone() draggable="true" data-entry=e.path.as_str().to_owned() data-folder=if folder{e.path.as_str().to_string()}else{String::new()}
        on:click=move |event|s.select(&click,event.ctrl_key()||event.meta_key(),event.shift_key()) on:dblclick=move |_|s.open(open.clone())
        on:contextmenu=move |event|{event.prevent_default();if !s.selected.get_untracked().iter().any(|p|p==menu.path.as_str()){s.select(&menu,false,false);}s.menu.set(Some((event.client_x(),event.client_y())));}>
        <div class="art-wrap"><FileArt entry=e.clone()/></div><div class="file-name"><strong title=e.name.clone()>{e.name.clone()}</strong><small>{if folder{"Dossier".into()}else{human_size(e.size)}}</small></div><span class="file-type">{if folder{"Dossier".into()}else{e.name.rsplit('.').next().unwrap_or("Fichier").to_uppercase()}}</span><span class="file-size">{if folder{"—".into()}else{human_size(e.size)}}</span><span class="file-date">{date(e.modified)}</span><button class="favorite-button" class:starred=move ||s.favorites.get().contains(&star_path) title="Ajouter ou retirer des favoris" on:click=move |event|{event.stop_propagation();s.favorite(fav.clone());}><Icon name="star"/></button>
    </article>}
}
#[component]
pub fn Inspector(state: State) -> impl IntoView {
    let s = state;
    view! {<aside class="inspector"><div class="inspector-title"><strong>"Détails"</strong><button title="Masquer les détails" on:click=move |_|s.details.set(false)>"×"</button></div>{move ||{
        let ids=s.selected.get();let e=s.entries.get().into_iter().find(|e|ids.contains(&e.path.as_str().to_string()));
        if let Some(e)=e{let open=e.clone();let favorite=e.clone();let mime=e.mime.clone();view!{<div class="inspector-art"><FileArt entry=e.clone()/></div><h2>{e.name.clone()}</h2><p class="detail-caption">{if e.kind==FileKind::Directory{"Dossier partagé".into()}else{mime}}</p><div class="detail-actions"><button class="primary" on:click=move |_|s.open(open.clone())>"Ouvrir"</button><button title="Favori" on:click=move |_|s.favorite(favorite.clone())><Icon name="star"/></button></div><dl><dt>"Taille"</dt><dd>{human_size(e.size)}</dd><dt>"Modifié le"</dt><dd>{date(e.modified)}</dd><dt>"Emplacement"</dt><dd class="detail-path">{e.path.as_str().to_string()}</dd><dt>"Partage"</dt><dd><span class="shared-dot"></span>"Toute l’équipe"</dd></dl><div class="inspector-links"><a href=download_url(&e.path,false)><Icon name="download"/>"Télécharger"</a><button on:click=move |_|s.ask("rename")>"Renommer"</button><button class="danger" on:click=move |_|s.ask("trash")><Icon name="trash"/>"Placer dans la corbeille"</button></div>}.into_any()}
        else{view!{<div class="inspector-empty"><div class="orb-file"><Icon name="file"/></div><h3>"Chaque fichier a son histoire"</h3><p>"Sélectionnez un élément pour afficher ses détails et les actions disponibles."</p></div><div class="keyboard-tip"><kbd>"⌘"</kbd><span>"ou Ctrl + clic pour sélectionner plusieurs éléments"</span></div>}.into_any()}
    }}</aside>}
}
#[component]
pub fn Dialogs(state: State) -> impl IntoView {
    let s = state;
    view! {<Show when=move ||s.dialog.get().is_some()><div class="modal-scrim"><form class="dialog" on:submit=move |e|{e.prevent_default();s.perform();}><div class="dialog-icon"><Icon name="folder"/></div><h2>{move ||match s.dialog.get().as_deref(){Some("trash")=>"Placer dans la corbeille ?",Some("rename")=>"Renommer l’élément",_=>"Un nouveau dossier"}}</h2><p>{move ||if s.dialog.get().as_deref()==Some("trash"){format!("{} élément(s) seront déplacés dans la corbeille. Vous pourrez les restaurer.",s.selected.get().len())}else{"Choisissez un nom pour votre espace partagé.".into()}}</p><Show when=move ||s.dialog.get().as_deref()!=Some("trash")><input autofocus required aria-label="Nom" maxlength="255" prop:value=move ||s.input.get() on:input=move |e|s.input.set(event_target_value(&e))/></Show><div class="dialog-actions"><button type="button" class="secondary" on:click=move |_|s.dialog.set(None)>"Annuler"</button><button type="submit" class="primary">"Confirmer"</button></div></form></div></Show>}
}
#[component]
pub fn ContextMenu(state: State) -> impl IntoView {
    let s = state;
    view! {<Show when=move ||s.menu.get().is_some()><div class="menu-dismiss" on:click=move |_|s.menu.set(None)></div><div class="context-menu" role="menu" style=move ||{let (x,y)=s.menu.get().unwrap_or((0,0));format!("left:min({x}px, calc(100vw - 230px));top:min({y}px, calc(100vh - 310px))") }>
        <button on:click=move |_|{s.menu.set(None);if let Some(e)=s.chosen().first(){s.open(e.clone());}}>"Ouvrir"<kbd>"↵"</kbd></button><button on:click=move |_|{s.clipboard.set(Some((s.chosen(),false)));s.menu.set(None);}>"Copier"<kbd>"⌘ C"</kbd></button><button on:click=move |_|{s.clipboard.set(Some((s.chosen(),true)));s.menu.set(None);}>"Couper"<kbd>"⌘ X"</kbd></button><button on:click=move |_|{s.paste(s.path.get_untracked());s.menu.set(None);}>"Coller ici"<kbd>"⌘ V"</kbd></button><hr/><button disabled=move ||s.selected.get().len()!=1 on:click=move |_|s.ask("rename")>"Renommer"<kbd>"F2"</kbd></button><button on:click=move |_|{for e in s.chosen(){s.favorite(e);}s.menu.set(None);}>"Ajouter / retirer des favoris"</button><hr/><button class="danger" on:click=move |_|s.ask("trash")>"Placer dans la corbeille"</button>
    </div></Show>}
}
#[component]
pub fn Trash(state: State) -> impl IntoView {
    let s = state;
    view! {<div class="trash-list">{move ||s.trash.get().into_iter().map(move |item|{let restore=item.clone();let purge=item.clone();view!{<div class="trash-row"><Icon name="trash"/><div><strong>{item.entry.name}</strong><small>{format!("{} · Supprimé le {}",item.original.as_str(),date(item.deleted))}</small></div><button class="secondary" on:click=move |_|{let item=restore.clone();spawn_local(async move{let result=api::send("POST",&format!("/api/v1/trash/{}/restore",item.id),&RestoreRequest{destination:item.original}).await;match result{Ok(_)=>s.reload(),Err(e)=>s.error.set(e)}});}>"Restaurer"</button><button class="danger" on:click=move |_|{if !web_sys::window().is_some_and(|w|w.confirm_with_message("Supprimer définitivement cet élément et son contenu ? Cette action est irréversible.").unwrap_or(false)){return;}let item=purge.clone();spawn_local(async move{match api::send("DELETE",&format!("/api/v1/trash/{}",item.id),&()).await{Ok(_)=>s.reload(),Err(e)=>s.error.set(e)}});}>"Supprimer"</button></div>}}).collect_view()}<Show when=move ||s.trash.get().is_empty()><div class="empty-state"><Icon name="trash"/><h2>"La corbeille est vide"</h2><p>"Les éléments que vous supprimez apparaîtront ici."</p></div></Show></div>}
}
#[component]
pub fn Preview(state: State) -> impl IntoView {
    let s = state;
    view! {<Show when=move ||s.preview.get().is_some()><div class="modal-scrim preview-scrim"><section class="preview-modal"><header><div><strong>{move ||s.preview.get().map(|e|e.name).unwrap_or_default()}</strong><small>{move ||s.preview.get().map(|e|human_size(e.size)).unwrap_or_default()}</small></div><div class="preview-actions"><Show when=move ||s.text.get().is_some()><button class="secondary" on:click=move |_|s.editing.update(|v|*v=!*v)>{move ||if s.editing.get(){"Aperçu"}else{"Modifier"}}</button><button class="primary" disabled=move ||s.saving.get() on:click=move |_|s.save()>{move ||if s.saving.get(){"Enregistrement…"}else{"Enregistrer"}}</button></Show><button title="Fermer l’aperçu" on:click=move |_|s.close_preview()>"×"</button></div></header>
    <Show when=move ||!s.error.get().is_empty()><div class="error-banner" role="alert"><span>{move ||s.error.get()}</span><button on:click=move |_|s.error.set(String::new())>"×"</button></div></Show><div class="preview-body">{move ||s.preview.get().map(|e|{
        let src=download_url(&e.path,true);
        if matches!(e.mime.as_str(),"image/png"|"image/jpeg"|"image/gif"|"image/webp"){view!{<div class="image-preview"><img src=src alt=e.name/></div>}.into_any()}
        else if e.mime.starts_with("video/"){view!{<video controls preload="metadata" src=src></video>}.into_any()}
        else if e.mime.starts_with("audio/"){view!{<audio controls preload="metadata" src=src></audio>}.into_any()}
        else if e.mime=="application/pdf"{view!{<iframe title="Aperçu PDF" sandbox="" src=src></iframe>}.into_any()}
        else if is_text(&e){view!{<Show when=move ||s.text.get().is_some() fallback=||view!{<div class="empty-state"><p>"Chargement du texte (limite : 1 Mio)…"</p></div>}><Show when=move ||s.editing.get() fallback=move ||view!{<div class="code-preview">{move ||s.draft.get().lines().enumerate().map(|(i,line)|view!{<div class="code-line"><span class="line-number">{i+1}</span><code>{highlight(line)}</code></div>}).collect_view()}</div>}><textarea class="code-editor" spellcheck="false" aria-label="Éditeur de texte" prop:value=move ||s.draft.get() on:input=move |e|s.draft.set(event_target_value(&e))></textarea></Show></Show>}.into_any()}
        else{view!{<div class="empty-state"><FileArt entry=e.clone()/><h2>"Ce format se découvre ailleurs"</h2><p>"Téléchargez le fichier pour l’ouvrir avec une application de votre ordinateur."</p><a class="primary" href=download_url(&e.path,false)>"Télécharger"</a></div>}.into_any()}
    })}</div><footer><span>{move ||if s.dirty(){"● Modifications non enregistrées"}else{"Aperçu sécurisé"}}</span><div><button class="text-button" on:click=move |_|{if let Some(w)=web_sys::window(){let _=w.alert_with_message("Pour VS Code : téléchargez le fichier, ouvrez-le sur votre poste puis réimportez votre version sous un nouveau nom. L’édition synchronisée est disponible dans GlassFTP ; aucun helper local n’est installé.");}}>"Ouvrir avec VS Code…"</button><a href=move ||s.preview.get().map(|e|download_url(&e.path,false)).unwrap_or_default()>"Télécharger"</a></div></footer></section></div></Show>}
}
/// Small, escaped lexical highlighter. Never inserts source text as HTML.
fn highlight(line: &str) -> impl IntoView + use<> {
    let mut words = vec![];
    let mut word = String::new();
    for ch in line.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            word.push(ch);
        } else {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            words.push(ch.to_string());
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
        .into_iter()
        .map(|w| {
            let class = if matches!(
                w.as_str(),
                "fn" | "let"
                    | "mut"
                    | "pub"
                    | "use"
                    | "import"
                    | "from"
                    | "def"
                    | "class"
                    | "return"
                    | "const"
                    | "function"
                    | "async"
                    | "await"
                    | "if"
                    | "else"
                    | "for"
                    | "while"
                    | "true"
                    | "false"
                    | "null"
                    | "SELECT"
                    | "FROM"
                    | "WHERE"
            ) {
                "token-keyword"
            } else if w.chars().all(|c| c.is_ascii_digit()) {
                "token-number"
            } else {
                ""
            };
            view! {<span class=class>{w}</span>}
        })
        .collect_view()
}
