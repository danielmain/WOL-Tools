//! Tauri 2 front-end ("OfflineWOL").
//!
//! Everything GUI-agnostic lives in `wol-core`; this crate only wires the
//! engine into Tauri: the `wol://` protocol, menus, windows and IPC commands.

pub mod bridge;
pub mod commands;

pub use wol_core::{
    cache, crawler, dirs, handler, http_types, jwlibrary_export, languages, publications, rewrite,
    state, sync, url_map,
};

use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use cache::DiskCache;
use crawler::Crawler;
use handler::ProtocolHandler;
use state::StateManager;
use sync::SyncManager;

pub fn dirs_fallback_app_data() -> std::path::PathBuf {
    dirs::app_data_dir_or_env()
}

fn to_tauri_response(
    response: wol_core::http_types::Response<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let (status, headers, body) = (
        response.status().as_u16(),
        response.headers().clone(),
        response.into_body(),
    );
    match tauri::http::Response::builder()
        .status(status)
        .body(body)
    {
        Ok(mut built) => {
            for (name, value) in headers.iter() {
                if let (Ok(name), Ok(value)) = (
                    tauri::http::header::HeaderName::try_from(name),
                    tauri::http::header::HeaderValue::try_from(value),
                ) {
                    built.headers_mut().insert(name, value);
                }
            }
            built
        }
        Err(_) => tauri::http::Response::new(Vec::new()),
    }
}

pub fn run_app() {
    let app_data_dir = dirs_fallback_app_data();
    let settings = StateManager::load_or_init(&app_data_dir)
        .map(|mgr| mgr.get_settings().unwrap_or_default())
        .unwrap_or_else(|err| {
            eprintln!("Failed to init state manager: {err}");
            std::process::exit(1);
        });
    let state_mgr = match StateManager::load_or_init(&app_data_dir) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("Failed to init state manager: {err}");
            std::process::exit(1);
        }
    };

    let cache_dir = app_data_dir.join("cache");
    let cache = match DiskCache::open(&cache_dir, settings.cache_cap_bytes) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("Failed to init disk cache: {err}");
            std::process::exit(1);
        }
    };

    let crawler = Crawler::new(cache.clone());
    let sync_mgr = SyncManager::new_shared(state_mgr.clone());

    let initial_marks = state_mgr
        .get_marks()
        .ok()
        .flatten()
        .and_then(|v| serde_json::to_string(&v).ok());
    let initial_cookies = state_mgr
        .get_cookies()
        .ok()
        .and_then(|c| serde_json::to_string(&c).ok());
    let init_script = bridge::build_injected_script_with_theme(
        initial_marks.as_deref(),
        initial_cookies.as_deref(),
        settings.theme,
    );

    let handler = Arc::new(ProtocolHandler::new_with_state(
        cache.clone(),
        state_mgr.clone(),
    ));

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state_mgr.clone())
        .manage(cache)
        .manage(crawler)
        .manage(sync_mgr.clone())
        .invoke_handler(tauri::generate_handler![
            commands::get_marks,
            commands::list_imported_publications_command,
            commands::save_imported_publication_command,
            commands::set_marks,
            commands::export_marks_to_file,
            commands::get_cookies,
            commands::set_cookie,
            commands::dispatch_tab_message,
            commands::get_settings,
            commands::save_settings,
            commands::get_languages,
            commands::navigate_home,
            commands::setup_from_url,
            commands::get_cache_stats,
            commands::list_cache_entries,
            commands::clear_cache,
            commands::clear_cache_older_than,
            commands::is_cached,
            commands::save_page_offline,
            commands::save_issue_offline,
            commands::save_bible_offline,
            commands::cancel_bible_offline,
            commands::save_library_offline,
            commands::cancel_library_offline,
            commands::open_manager_window,
            commands::get_sync_status,
            commands::sync_now,
            commands::connect_nextcloud,
            commands::connect_nextcloud_browser,
            commands::connect_google_drive,
            commands::disconnect_sync,
        ])
        .register_asynchronous_uri_scheme_protocol("wol", move |_app, request, responder| {
            let handler = handler.clone();
            tauri::async_runtime::spawn(async move {
                let resp = match wol_core::url_map::Url::parse(&request.uri().to_string()) {
                    Ok(url) => handler.handle_request(&url, request.method()).await,
                    Err(err) => wol_core::http_types::Response::new(
                        wol_core::http_types::StatusCode::BAD_REQUEST,
                        wol_core::http_types::Headers::new(),
                        format!("Bad request URI: {err}").into_bytes(),
                    ),
                };
                responder.respond(to_tauri_response(resp));
            });
        })
        .setup({
            let sync_mgr = sync_mgr.clone();
            move |app| {
                install_menu(app)?;
                install_sync_listener(app, &sync_mgr);
                build_main_window(app, &settings, &init_script)?;
                spawn_background_sync(app, &sync_mgr);
                Ok(())
            }
        });

    if let Err(err) = builder.run(tauri::generate_context!()) {
        eprintln!("Error running application: {err}");
        std::process::exit(1);
    }
}

fn install_menu(app: &mut tauri::App) -> tauri::Result<()> {
    let manage_item = MenuItem::with_id(
        app,
        "manage_highlights",
        "Manage Highlights & Settings...",
        true,
        Some("CmdOrControl+Shift+H"),
    )?;
    let app_submenu = Submenu::with_items(
        app,
        "OfflineWOL",
        true,
        &[
            &manage_item,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    let edit_submenu = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let back_item = MenuItem::with_id(app, "nav_back", "Back", true, Some("CmdOrControl+["))?;
    let forward_item = MenuItem::with_id(
        app,
        "nav_forward",
        "Forward",
        true,
        Some("CmdOrControl+]"),
    )?;
    let history_submenu =
        Submenu::with_items(app, "History", true, &[&back_item, &forward_item])?;
    let menu = Menu::with_items(app, &[&app_submenu, &edit_submenu, &history_submenu])?;
    app.set_menu(menu)?;

    app.on_menu_event(|app, event| {
        if event.id() == "manage_highlights" {
            let _ = commands::open_manager_window(app.clone(), None);
        } else if event.id() == "nav_back" {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.eval("window.history.back()");
            }
        } else if event.id() == "nav_forward" {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.eval("window.history.forward()");
            }
        }
    });
    Ok(())
}

fn install_sync_listener(app: &tauri::App, sync_mgr: &SyncManager) {
    let handle = app.handle().clone();
    sync_mgr.set_listener(Some(Arc::new(move || {
        if let Some(main_win) = handle.get_webview_window("main") {
            let _ = main_win.eval(
                "if (window.__wolDispatchMessage) { window.__wolDispatchMessage({ type: 'wol-reapply' }); }",
            );
        }
        if let Some(mgr_win) = handle.get_webview_window("manager") {
            let _ = mgr_win.eval("if (window.__wolOnSyncUpdated) { window.__wolOnSyncUpdated(); }");
        }
    })));
}

fn build_main_window(
    app: &tauri::App,
    settings: &state::AppSettings,
    init_script: &str,
) -> tauri::Result<()> {
    let initial_url = initial_webview_url(settings);
    let navigation_app = app.handle().clone();
    WebviewWindowBuilder::new(app, "main", initial_url)
        .title(if settings.is_configured() {
            "OfflineWOL"
        } else {
            "OfflineWOL — Setup"
        })
        .inner_size(1200.0, 800.0)
        .theme(commands::theme_to_tauri(settings.theme))
        .initialization_script(
            r#"
            window.addEventListener('click', function (event) {
                if (event.button !== 0 || event.defaultPrevented) return;
                const element = event.target instanceof Element ? event.target : event.target?.parentElement;
                const link = element?.closest('a[href]');
                if (!link || link.hasAttribute('download')) return;
                const target = (link.getAttribute('target') || '').toLowerCase();
                if (target !== '_blank' && !link.hasAttribute('data-video')) return;
                let url;
                try { url = new URL(link.href, document.baseURI); } catch (_) { return; }
                if (!['http:', 'https:', 'wol:'].includes(url.protocol)) return;
                event.preventDefault();
                event.stopImmediatePropagation();
                window.location.assign(url.href);
            }, true);
        "#,
        )
        .initialization_script(init_script)
        .on_new_window(move |url, _features| {
            if matches!(url.scheme(), "http" | "https" | "wol") {
                if let Some(window) = navigation_app.get_webview_window("main") {
                    let _ = window.navigate(url);
                }
            }
            tauri::webview::NewWindowResponse::Deny
        })
        .build()?;
    Ok(())
}

fn initial_webview_url(settings: &state::AppSettings) -> WebviewUrl {
    if !settings.is_configured() {
        return WebviewUrl::App("setup.html".into());
    }
    let lib = languages::find_by_locale(&settings.language)
        .or_else(|| languages::find_by_symbol(&settings.language))
        .map(|l| l.lib.clone())
        .unwrap_or_else(|| settings.language.clone());
    let start_wol_url = format!(
        "wol://{}/wol/h/r{}/lp-{}",
        settings.language, settings.region, lib
    );
    match url::Url::parse(&start_wol_url) {
        Ok(parsed) => WebviewUrl::CustomProtocol(parsed),
        Err(_) => WebviewUrl::App("setup.html".into()),
    }
}

fn spawn_background_sync(app: &tauri::App, sync_mgr: &SyncManager) {
    let bg_sync = sync_mgr.clone();
    let _handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        loop {
            let _ = bg_sync.sync_now().await;
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        }
    });
}
