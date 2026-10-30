mod cache;
mod commands;
mod pipeline;
#[cfg(windows)]
mod resize_outline;
mod result_resize;
mod result_toolbar;
mod result_tools;
mod shortcuts;
mod state;
mod theme_preview;
#[cfg(windows)]
mod update_caption;
mod update_ui;
mod updater;
#[cfg(windows)]
use update_caption::set_update_caption;
#[cfg(not(windows))]
use update_ui::set_update_caption;
#[cfg(all(test, windows, feature = "e2e"))]
mod background_tests;
mod visual_render;
mod windows;

use state::{AppState, lock};
use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder},
};

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, arguments, _| {
            if arguments.iter().any(|arg| arg == "--capture") {
                windows::begin_capture(app.clone(), None);
            } else if arguments.iter().any(|arg| arg == "--quick") {
                let _ = windows::toggle_quick(app);
            } else if arguments.iter().any(|arg| arg == "--toggle-result") {
                let _ = windows::toggle_result(app);
            } else {
                let _ = windows::show_main(app);
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--background"])
                .build(),
        )
        .register_uri_scheme_protocol("frame", |context, request| {
            let id = request.uri().path().trim_start_matches('/');
            let state = context.app_handle().state::<AppState>();
            let png = if id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                lock(&state.frames).png(id)
            } else {
                None
            };
            let origin = request
                .headers()
                .get("Origin")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("");
            let trusted_origin = matches!(
                origin,
                "http://tauri.localhost"
                    | "https://tauri.localhost"
                    | "tauri://localhost"
                    | "http://127.0.0.1:1420"
                    | "http://localhost:1420"
            );
            match png {
                Some(png) => {
                    let mut response = tauri::http::Response::builder()
                        .status(200)
                        .header("Content-Type", "image/png")
                        .header("Cache-Control", "no-store")
                        .header("Vary", "Origin");
                    if trusted_origin {
                        response = response.header("Access-Control-Allow-Origin", origin);
                    }
                    response.body(png.as_ref().clone()).unwrap()
                }
                None => tauri::http::Response::builder()
                    .status(404)
                    .body(Vec::new())
                    .unwrap(),
            }
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                windows::window_destroyed(window);
            }
            if matches!(event, tauri::WindowEvent::ThemeChanged(_)) {
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let saved = lock(&state.config).appearance.theme;
                    let theme = lock(&state.theme_preview).effective(saved);
                    windows::apply_theme(&app, theme);
                });
            }
            if matches!(
                event,
                tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)
            ) {
                result_tools::window_changed(window);
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                windows::close_requested(window, api);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::begin_result_drag,
            commands::finish_result_drag,
            commands::begin_result_resize,
            commands::update_result_resize,
            commands::bootstrap,
            commands::window_ready,
            commands::result_presented,
            commands::selection_presented,
            commands::refresh_capabilities,
            commands::preview_theme,
            commands::set_text,
            commands::translate_text,
            commands::cancel_job,
            commands::begin_capture,
            commands::cancel_capture,
            commands::selection_frame,
            commands::select_region,
            commands::retry_visual,
            commands::edit_block,
            commands::copy_text,
            commands::show_main,
            commands::set_route,
            commands::navigate_main,
            commands::toggle_quick,
            commands::toggle_result,
            commands::result_layout,
            commands::set_result_toolbar,
            commands::toolbar_placement,
            commands::begin_toolbar_drag,
            commands::finish_toolbar_drag,
            commands::quiet_result_toolbar,
            commands::result_display,
            commands::set_result_display,
            commands::show_result_notice,
            commands::hide_window,
            commands::nudge_result,
            commands::save_config,
            commands::set_dirty,
            commands::save_secret,
            commands::read_secret,
            commands::delete_secret,
            commands::credential_status,
            commands::test_provider,
            commands::list_models,
            commands::import_legacy_config,
            commands::open_image,
            commands::configure_portal_shortcuts,
            commands::open_help,
            updater::check_update,
            updater::download_update,
            updater::install_update,
            update_ui::set_tray_update,
            update_ui::take_update_request,
            set_update_caption,
            commands::quit,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            app.manage(updater::PendingUpdate::default());
            app.manage(update_ui::UpdateUi::default());
            let key = updater::public_key();
            if !key.is_empty() {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().pubkey(key).build())?;
            }
            app.manage(AppState::new(&handle));
            let state = handle.state::<AppState>();
            let keys = lock(&state.config).hotkeys.clone();
            if !cfg!(feature = "e2e")
                && let Err(error) = shortcuts::apply(&handle, &keys)
            {
                state.warning(&handle, error.message);
            }
            if !cfg!(feature = "e2e")
                && screentrans_platform::is_wayland()
                && lock(&state.config).wayland_shortcuts
            {
                shortcuts::configure_portal(handle.clone(), keys);
            }
            if !cfg!(feature = "e2e")
                && let Err(error) = setup_tray(&handle)
            {
                lock(&state.capabilities).tray = false;
                state.warning(&handle, format!("系统托盘不可用，将保留主窗口：{error}"));
            }
            let arguments: Vec<_> = std::env::args().collect();
            let background = arguments.iter().any(|arg| arg == "--background")
                && lock(&state.capabilities).tray
                && lock(&state.config).onboarding_complete;
            if arguments.iter().any(|arg| arg == "--capture") {
                windows::begin_capture(handle.clone(), None);
            } else if arguments.iter().any(|arg| arg == "--quick") {
                let _ = windows::toggle_quick(&handle);
            } else if !background {
                windows::show_main(&handle)?;
            }
            windows::reap_hidden_windows(handle);
            Ok(())
        });
    #[cfg(feature = "e2e")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());
    builder
        .build(tauri::generate_context!())
        .expect("ScreenTranslate could not initialize its desktop runtime")
        .run(handle_run_event);
}

fn handle_run_event(_app: &tauri::AppHandle, event: tauri::RunEvent) {
    // Reaping the last hidden webview must leave tray and shortcut handlers alive.
    // Explicit exit/restart requests carry a code and must still be honored.
    if let tauri::RunEvent::ExitRequested {
        code: None, api, ..
    } = event
    {
        api.prevent_exit();
    }
}

fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let version = MenuItem::with_id(
        app,
        "update",
        format!("版本 {}", env!("CARGO_PKG_VERSION")),
        false,
        None::<&str>,
    )?;
    *lock(&app.state::<update_ui::UpdateUi>().tray) = Some(version.clone());
    let capture = MenuItem::with_id(app, "capture", "截图翻译", true, None::<&str>)?;
    let quick = MenuItem::with_id(app, "quick", "快速文字翻译", true, None::<&str>)?;
    let main = MenuItem::with_id(app, "main", "打开主窗口", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "显示 / 隐藏译文", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&version, &capture, &quick, &main, &toggle, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main-tray")
        .tooltip("ScreenTranslate · 截图即翻译")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "update" => update_ui::request(app, "tray"),
            "main" => {
                let _ = windows::show_main(app);
            }
            "quit" => {
                app.state::<AppState>().jobs.cancel_all();
                app.exit(0);
            }
            id => shortcuts::dispatch(app, id),
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                tauri::tray::TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                windows::begin_capture(tray.app_handle().clone(), None);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    let _ = app.emit("tray-ready", ());
    Ok(())
}
