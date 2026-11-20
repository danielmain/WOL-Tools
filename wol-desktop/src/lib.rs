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

use std::sync::{Arc, OnceLock};
#[cfg(desktop)]
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

fn resolve_app_data_dir(app: &tauri::App) -> std::path::PathBuf {
    #[cfg(mobile)]
    {
        if let Ok(dir) = app.path().app_data_dir() {
            dirs::set_app_data_dir_override(dir.clone());
            return dir;
        }
    }
    #[cfg(not(mobile))]
    let _ = app;
    dirs_fallback_app_data()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run_app() {
    let protocol_handler: Arc<OnceLock<Arc<ProtocolHandler>>> = Arc::new(OnceLock::new());

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_marks,
            commands::list_imported_publications,
            commands::save_imported_publication,
            commands::set_marks,
            commands::export_marks_to_file,
            commands::export_marks_to_bytes,
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
        .register_asynchronous_uri_scheme_protocol("wol", {
            let protocol_handler = protocol_handler.clone();
            move |_app, request, responder| {
                let protocol_handler = protocol_handler.clone();
                tauri::async_runtime::spawn(async move {
                    let resp = match protocol_handler.get() {
                        Some(handler) => {
                            match wol_core::url_map::Url::parse(&request.uri().to_string()) {
                                Ok(url) => handler.handle_request(&url, request.method()).await,
                                Err(err) => wol_core::http_types::Response::new(
                                    wol_core::http_types::StatusCode::BAD_REQUEST,
                                    wol_core::http_types::Headers::new(),
                                    format!("Bad request URI: {err}").into_bytes(),
                                ),
                            }
                        }
                        None => wol_core::http_types::Response::new(
                            wol_core::http_types::StatusCode::SERVICE_UNAVAILABLE,
                            wol_core::http_types::Headers::new(),
                            b"Protocol handler not initialized".to_vec(),
                        ),
                    };
                    responder.respond(to_tauri_response(resp));
                });
            }
        })
        .setup({
            let protocol_handler = protocol_handler.clone();
            move |app| {
                let app_data_dir = resolve_app_data_dir(app);
                let state_mgr = StateManager::load_or_init(&app_data_dir).map_err(|err| {
                    std::io::Error::other(format!("Failed to init state manager: {err}"))
                })?;
                let settings = state_mgr.get_settings().unwrap_or_default();

                let cache_dir = app_data_dir.join("cache");
                let cache = DiskCache::open(&cache_dir, settings.cache_cap_bytes).map_err(|err| {
                    std::io::Error::other(format!("Failed to init disk cache: {err}"))
                })?;

                let crawler = Crawler::new(cache.clone());
                let sync_mgr = SyncManager::new_shared(state_mgr.clone());
                install_sync_openers(app, &sync_mgr);

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
                let _ = protocol_handler.set(handler);

                app.manage(state_mgr);
                app.manage(cache);
                app.manage(crawler);
                app.manage(sync_mgr.clone());

                #[cfg(desktop)]
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

#[cfg(desktop)]
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
                "if (window.__wolDispatchMessage) { window.__wolDispatchMessage({ type: 'wol-reapply' }); } if (window.__wolOnSyncUpdated) { window.__wolOnSyncUpdated(); }",
            );
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
    #[cfg_attr(mobile, allow(unused_mut))]
    let mut builder = WebviewWindowBuilder::new(app, "main", initial_url);
    #[cfg(desktop)]
    {
        builder = builder
            .title(if settings.is_configured() {
                "OfflineWOL"
            } else {
                "OfflineWOL — Setup"
            })
            .inner_size(1200.0, 800.0)
            .theme(commands::theme_to_tauri(settings.theme));
    }
    builder
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

fn install_sync_openers(app: &tauri::App, sync_mgr: &SyncManager) {
    use tauri_plugin_opener::OpenerExt;

    let app_handle = app.handle().clone();
    sync_mgr.set_browser_opener(Some(Arc::new(move |url: &str| {
        app_handle
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| format!("Failed to open browser: {e}"))
    })));

    #[cfg(target_os = "ios")]
    sync_mgr.set_oauth_session_opener(Some(Arc::new(
        |auth_url: &str, callback_scheme: &str| {
            let auth_url = auth_url.to_string();
            let callback_scheme = callback_scheme.to_string();
            Box::pin(async move { ios_oauth_session(&auth_url, &callback_scheme).await })
        },
    )));
}

#[cfg(target_os = "ios")]
async fn ios_oauth_session(auth_url: &str, callback_scheme: &str) -> Result<String, String> {
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_void};

    extern "C" {
        fn wol_ios_start_oauth_session(
            auth_url: *const c_char,
            callback_scheme: *const c_char,
            ctx: *mut c_void,
            completion: extern "C" fn(*mut c_void, *const c_char, *const c_char),
        );
    }

    extern "C" fn on_oauth_complete(
        ctx: *mut c_void,
        callback_url: *const c_char,
        error: *const c_char,
    ) {
        let tx = unsafe { Box::from_raw(ctx as *mut tokio::sync::oneshot::Sender<Result<String, String>>) };
        let res = if !callback_url.is_null() {
            Ok(unsafe { CStr::from_ptr(callback_url) }
                .to_string_lossy()
                .into_owned())
        } else if !error.is_null() {
            Err(unsafe { CStr::from_ptr(error) }
                .to_string_lossy()
                .into_owned())
        } else {
            Err("OAuth sign-in cancelled.".to_string())
        };
        let _ = tx.send(res);
    }

    let c_url = CString::new(auth_url).map_err(|_| "Invalid OAuth URL.".to_string())?;
    let c_scheme =
        CString::new(callback_scheme).map_err(|_| "Invalid OAuth callback scheme.".to_string())?;
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let ctx = Box::into_raw(Box::new(tx)) as *mut c_void;

    unsafe {
        wol_ios_start_oauth_session(c_url.as_ptr(), c_scheme.as_ptr(), ctx, on_oauth_complete);
    }

    rx.await
        .map_err(|_| "OAuth session channel closed unexpectedly.".to_string())?
}
