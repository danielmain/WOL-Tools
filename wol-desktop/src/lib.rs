pub mod bridge;
pub mod cache;
pub mod commands;
pub mod crawler;
pub mod handler;
pub mod languages;
pub mod rewrite;
pub mod state;
pub mod url_map;

use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use cache::DiskCache;
use crawler::Crawler;
use handler::ProtocolHandler;
use state::StateManager;

pub fn dirs_fallback_app_data() -> std::path::PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        let app_support = std::path::PathBuf::from(home)
            .join("Library")
            .join("Application Support");
        let new_dir = app_support.join("com.offlinewol.desktop");
        let legacy_dir = app_support.join("com.wolhighlights.desktop");
        if !new_dir.exists() && legacy_dir.exists() {
            let _ = std::fs::rename(&legacy_dir, &new_dir);
        }
        new_dir
    } else {
        std::path::PathBuf::from(".offline-wol-data")
    }
}

pub fn run_app() {
    let app_data_dir = dirs_fallback_app_data();
    let state_mgr = match StateManager::load_or_init(&app_data_dir) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("Failed to init state manager: {err}");
            std::process::exit(1);
        }
    };

    let settings = state_mgr.get_settings().unwrap_or_default();
    let cache_dir = app_data_dir.join("cache");
    let cache = match DiskCache::open(&cache_dir, settings.cache_cap_bytes) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("Failed to init disk cache: {err}");
            std::process::exit(1);
        }
    };

    let crawler = Crawler::new(cache.clone());

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
        .manage(state_mgr.clone())
        .manage(cache)
        .manage(crawler)
        .invoke_handler(tauri::generate_handler![
            commands::get_marks,
            commands::set_marks,
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
        ])
        .register_asynchronous_uri_scheme_protocol("wol", move |_app, request, responder| {
            let handler = handler.clone();
            tauri::async_runtime::spawn(async move {
                let resp = handler.handle_request(request.uri(), request.method()).await;
                responder.respond(resp);
            });
        })
        .setup(move |app| {
            // Build application menu
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
            let back_item = MenuItem::with_id(
                app,
                "nav_back",
                "Back",
                true,
                Some("CmdOrControl+["),
            )?;
            let forward_item = MenuItem::with_id(
                app,
                "nav_forward",
                "Forward",
                true,
                Some("CmdOrControl+]"),
            )?;
            let history_submenu = Submenu::with_items(
                app,
                "History",
                true,
                &[&back_item, &forward_item],
            )?;
            let menu = Menu::with_items(app, &[&app_submenu, &edit_submenu, &history_submenu])?;
            app.set_menu(menu)?;

            app.on_menu_event(|app, event| {
                if event.id() == "manage_highlights" {
                    let _ = commands::open_manager_window(app.clone());
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

            // Start URL based on settings: show setup wizard on first run
            let (initial_url, initial_title) = if settings.is_configured() {
                let lang_info = languages::find_by_locale(&settings.language)
                    .or_else(|| languages::find_by_symbol(&settings.language));
                let lib = lang_info
                    .map(|l| l.lib.as_str())
                    .unwrap_or(&settings.language);

                let start_wol_url = format!(
                    "wol://{}/wol/h/r{}/lp-{}",
                    settings.language, settings.region, lib
                );
                match url::Url::parse(&start_wol_url) {
                    Ok(parsed) => (WebviewUrl::CustomProtocol(parsed), "OfflineWOL"),
                    Err(_) => (WebviewUrl::App("setup.html".into()), "OfflineWOL — Setup"),
                }
            } else {
                (WebviewUrl::App("setup.html".into()), "OfflineWOL — Setup")
            };

            let _win = WebviewWindowBuilder::new(app, "main", initial_url)
                .title(initial_title)
                .inner_size(1200.0, 800.0)
                .theme(settings.theme.to_tauri_theme())
                .initialization_script(&init_script)
                .build()?;
            Ok(())
        });

    if let Err(err) = builder.run(tauri::generate_context!()) {
        eprintln!("Error running application: {err}");
        std::process::exit(1);
    }
}
