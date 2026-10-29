use crate::{
    pipeline, shortcuts,
    state::{AppState, lock},
    windows,
};
use screentrans_core::{
    AppError, Bootstrap, CaptureFrame, ErrorCode, FrameMetadata, JobStage, MigrationReport,
    PlatformCapabilities, Rect, Result, TranslationSession,
    config::{Config, LANGUAGES, ProviderId, import_legacy},
    ports::TranslatorBackend,
    translators::HttpTranslator,
};
use std::collections::BTreeMap;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_autostart::ManagerExt;
#[cfg(not(feature = "e2e"))]
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tokio_util::sync::CancellationToken;

fn main_only(window: &WebviewWindow) -> Result<()> {
    if window.label() != "main" {
        return Err(AppError::new(
            ErrorCode::PermissionDenied,
            "请在主窗口修改设置",
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn begin_result_drag(window: WebviewWindow) -> Result<()> {
    if window.label() != "result" {
        return Err(AppError::new(
            ErrorCode::PermissionDenied,
            "请在截图窗口移动截图",
        ));
    }
    crate::result_tools::begin_image_drag(window.app_handle())
}

#[tauri::command]
pub async fn finish_result_drag(window: WebviewWindow) -> Result<()> {
    if window.label() != "result" {
        return Err(AppError::new(
            ErrorCode::PermissionDenied,
            "请在截图窗口移动截图",
        ));
    }
    crate::result_tools::finish_image_drag(&window).await
}

#[tauri::command]
pub async fn begin_result_resize(
    window: WebviewWindow,
    direction: crate::result_resize::ResizeDirection,
) -> Result<()> {
    if window.label() != "result" {
        return Err(AppError::new(
            ErrorCode::PermissionDenied,
            "请在截图窗口调整大小",
        ));
    }
    windows::begin_result_resize(&window, direction)
}

#[tauri::command]
pub async fn update_result_resize(window: WebviewWindow, finish: bool) -> Result<Option<Rect>> {
    if window.label() != "result" {
        return Err(AppError::new(
            ErrorCode::PermissionDenied,
            "请在截图窗口调整大小",
        ));
    }
    windows::update_result_resize(&window, finish)
}

#[tauri::command]
pub async fn window_ready(window: WebviewWindow, frame_id: Option<String>) -> Result<()> {
    windows::window_ready(&window, frame_id.as_deref())
}

#[tauri::command]
pub async fn result_presented(window: WebviewWindow, frame_id: String) -> Result<()> {
    windows::result_presented(&window, &frame_id);
    Ok(())
}

#[tauri::command]
pub async fn selection_presented(window: WebviewWindow) -> Result<()> {
    windows::selection_presented(&window);
    Ok(())
}

#[tauri::command]
pub async fn bootstrap(app: AppHandle) -> Result<Bootstrap> {
    Ok(app.state::<AppState>().bootstrap())
}

#[tauri::command]
pub async fn refresh_capabilities(app: AppHandle) -> Result<PlatformCapabilities> {
    let mut capabilities = tauri::async_runtime::spawn_blocking(screentrans_platform::capabilities)
        .await
        .map_err(|_| AppError::new(ErrorCode::Internal, "系统能力检查失败"))?;
    let state = app.state::<AppState>();
    if capabilities.wayland {
        capabilities.global_shortcuts = lock(&state.capabilities).global_shortcuts;
    }
    *lock(&state.capabilities) = capabilities.clone();
    Ok(capabilities)
}

#[tauri::command]
pub async fn preview_theme(
    app: AppHandle,
    window: WebviewWindow,
    theme: screentrans_core::config::Theme,
) -> Result<()> {
    main_only(&window)?;
    let state = app.state::<AppState>();
    let saved = lock(&state.config).appearance.theme;
    let changed = lock(&state.theme_preview).preview(saved, theme);
    if changed {
        windows::apply_theme(&app, theme);
        let _ = app.emit("theme-previewed", theme);
    }
    Ok(())
}

#[tauri::command]
pub async fn set_text(
    app: AppHandle,
    input: String,
    source: String,
    target: String,
    composing: bool,
    client_id: String,
    client_revision: u32,
) -> Result<TranslationSession> {
    if input.len() > 100_000
        || client_id.len() > 80
        || !LANGUAGES.contains(&source.as_str())
        || !LANGUAGES.contains(&target.as_str())
    {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "输入文字过长或语言配置无效",
        ));
    }
    let state = app.state::<AppState>();
    {
        let mut sessions = lock(&state.sessions);
        if sessions
            .clients
            .get(&client_id)
            .is_some_and(|previous| *previous >= client_revision)
        {
            return Ok(sessions.text.clone());
        }
        if sessions.clients.len() > 64 {
            sessions.clients.clear();
        }
        sessions.clients.insert(client_id.clone(), client_revision);
        state.jobs.cancel("text");
        sessions.text.revision += 1;
        sessions.text.client_id = client_id;
        sessions.text.client_revision = client_revision;
        sessions.text.input = input;
        sessions.text.source = source;
        sessions.text.target = target;
        sessions.text.output.clear();
        sessions.text.job.stage = JobStage::Idle;
        sessions.text.job.error = None;
    }
    if !composing && lock(&state.config).auto_translate {
        pipeline::schedule_text(app.clone(), true);
    }
    state.publish_text(&app);
    Ok(lock(&state.sessions).text.clone())
}

#[tauri::command]
pub async fn translate_text(app: AppHandle, revision: u32) -> Result<()> {
    let state = app.state::<AppState>();
    if lock(&state.sessions).text.revision != revision {
        return Err(AppError::new(
            ErrorCode::Cancelled,
            "输入已更新，请重新翻译",
        ));
    }
    pipeline::schedule_text(app, false);
    Ok(())
}

#[tauri::command]
pub async fn cancel_job(app: AppHandle, session: String) -> Result<()> {
    let state = app.state::<AppState>();
    if session != "text" && session != "visual" {
        return Err(AppError::new(ErrorCode::InvalidInput, "未知会话"));
    }
    state.jobs.cancel(&session);
    if session == "visual" {
        state.jobs.cancel_prefix("block:");
    }
    {
        let mut sessions = lock(&state.sessions);
        let session = if session == "text" {
            &mut sessions.text
        } else {
            &mut sessions.visual
        };
        session.job.stage = JobStage::Cancelled;
        session.job.error = None;
    }
    if session == "text" {
        state.publish_text(&app);
    } else {
        state.publish_visual(&app);
    }
    Ok(())
}

#[tauri::command]
pub async fn begin_capture(app: AppHandle, monitor: Option<String>) -> Result<()> {
    windows::begin_capture(app, monitor);
    Ok(())
}

#[tauri::command]
pub async fn cancel_capture(app: AppHandle) -> Result<()> {
    windows::cancel_capture(&app, true);
    Ok(())
}

#[tauri::command]
pub async fn selection_frame(app: AppHandle, window: WebviewWindow) -> Result<FrameMetadata> {
    let state = app.state::<AppState>();
    let id = lock(&state.selections)
        .get(window.label())
        .cloned()
        .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "选区已过期"))?;
    lock(&state.frames).metadata(&id)
}

#[tauri::command]
pub async fn select_region(app: AppHandle, window: WebviewWindow, rect: Rect) -> Result<()> {
    windows::accept_selection(&app, window.label(), rect)
}

#[tauri::command]
pub async fn retry_visual(app: AppHandle) -> Result<()> {
    let (metadata, recognize) = {
        let state = app.state::<AppState>();
        let sessions = lock(&state.sessions);
        (
            sessions
                .visual
                .frame
                .clone()
                .ok_or_else(|| AppError::new(ErrorCode::InvalidInput, "请先截图"))?,
            sessions.visual.blocks.is_empty(),
        )
    };
    pipeline::start_visual(app, metadata, recognize);
    Ok(())
}

#[tauri::command]
pub async fn edit_block(app: AppHandle, id: String, text: String, target: String) -> Result<()> {
    if text.trim().is_empty() || text.len() > 30_000 || !LANGUAGES.contains(&target.as_str()) {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "请输入有效文字并选择目标语言",
        ));
    }
    pipeline::retranslate_block(app, id, text, target)
}

#[tauri::command]
pub async fn copy_text(app: AppHandle, text: String) -> Result<()> {
    if text.len() > 500_000 {
        return Err(AppError::new(ErrorCode::InvalidInput, "复制内容过长"));
    }
    #[cfg(feature = "e2e")]
    {
        // UI regression tests must not overwrite the host user's clipboard.
        let _ = app;
        Ok(())
    }
    #[cfg(not(feature = "e2e"))]
    {
        app.clipboard()
            .write_text(text)
            .map_err(|_| AppError::new(ErrorCode::Internal, "无法写入系统剪贴板"))
    }
}

#[tauri::command]
pub async fn show_main(app: AppHandle) -> Result<()> {
    windows::show_main(&app)
}

fn validate_route(route: &str) -> Result<()> {
    if ![
        "shortcuts",
        "translate",
        "capture",
        "providers",
        "ocr",
        "settings",
        "about",
    ]
    .contains(&route)
    {
        return Err(AppError::new(ErrorCode::InvalidInput, "未知页面"));
    }
    Ok(())
}

#[tauri::command]
pub async fn set_route(app: AppHandle, window: WebviewWindow, route: String) -> Result<()> {
    main_only(&window)?;
    validate_route(&route)?;
    *lock(&app.state::<AppState>().route) = route;
    Ok(())
}

#[tauri::command]
pub async fn navigate_main(app: AppHandle, route: String) -> Result<()> {
    validate_route(&route)?;
    *lock(&app.state::<AppState>().route) = route.clone();
    windows::show_main(&app)?;
    let _ = app.emit_to("main", "navigate", route);
    Ok(())
}

#[tauri::command]
pub async fn toggle_quick(app: AppHandle) -> Result<()> {
    windows::toggle_quick(&app)
}

#[tauri::command]
pub async fn result_layout(app: AppHandle) -> Result<windows::ResultLayout> {
    Ok(lock(&app.state::<AppState>().result_layout).clone())
}

#[tauri::command]
pub async fn set_result_toolbar(
    window: WebviewWindow,
    above: bool,
    align_left: bool,
) -> Result<windows::ResultLayout> {
    if !matches!(window.label(), "result" | "result-tools") {
        return Err(AppError::new(
            ErrorCode::PermissionDenied,
            "请在截图窗口调整工具栏",
        ));
    }
    windows::set_result_toolbar(&window, above, align_left)
}

fn result_only(window: &WebviewWindow) -> Result<()> {
    if !matches!(window.label(), "result" | "result-tools") {
        return Err(AppError::new(
            ErrorCode::PermissionDenied,
            "请在截图窗口操作",
        ));
    }
    Ok(())
}
#[tauri::command]
pub async fn toolbar_placement(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<crate::result_toolbar::ToolbarState> {
    result_only(&window)?;
    Ok(lock(&app.state::<AppState>().result_toolbar).clone())
}
#[tauri::command]
pub async fn begin_toolbar_drag(app: AppHandle, window: WebviewWindow) -> Result<()> {
    result_only(&window)?;
    crate::result_tools::begin_drag(&app);
    Ok(())
}
#[tauri::command]
pub async fn finish_toolbar_drag(app: AppHandle, window: WebviewWindow) -> Result<()> {
    result_only(&window)?;
    crate::result_tools::clamp_toolbar(&app)
}
#[tauri::command]
pub async fn quiet_result_toolbar(
    app: AppHandle,
    window: WebviewWindow,
    quiet: bool,
) -> Result<()> {
    result_only(&window)?;
    crate::result_tools::quiet(&app, quiet)
}
#[tauri::command]
pub async fn result_display(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<crate::result_toolbar::ResultDisplay> {
    result_only(&window)?;
    Ok(lock(&app.state::<AppState>().result_display).clone())
}
#[tauri::command]
pub async fn set_result_display(
    app: AppHandle,
    window: WebviewWindow,
    original: bool,
    comparing: bool,
    holding_original: bool,
) -> Result<()> {
    result_only(&window)?;
    let state = app.state::<AppState>();
    let frame_id = lock(&state.result_frame_id).clone();
    let display = crate::result_toolbar::ResultDisplay {
        frame_id,
        original,
        comparing,
        holding_original,
    };
    *lock(&state.result_display) = display.clone();
    let _ = app.emit("result-display-changed", display);
    Ok(())
}
#[tauri::command]
pub async fn show_result_notice(
    app: AppHandle,
    window: WebviewWindow,
    text: String,
    tone: String,
) -> Result<()> {
    result_only(&window)?;
    let text: String = text.chars().take(80).collect();
    let _ = app.emit_to(
        "result",
        "result-feedback",
        serde_json::json!({"text":text,"tone":if tone=="error"{"error"}else{"normal"}}),
    );
    Ok(())
}

#[tauri::command]
pub async fn toggle_result(app: AppHandle) -> Result<()> {
    windows::toggle_result(&app)
}

#[tauri::command]
pub async fn hide_window(window: WebviewWindow) -> Result<()> {
    if matches!(window.label(), "result" | "result-tools") {
        return crate::result_tools::hide(window.app_handle());
    }
    window
        .hide()
        .map_err(|_| AppError::new(ErrorCode::Internal, "无法收起窗口"))
}

#[tauri::command]
pub async fn nudge_result(window: WebviewWindow, dx: i32, dy: i32) -> Result<()> {
    if !matches!(window.label(), "result" | "result-tools")
        || !(-100..=100).contains(&dx)
        || !(-100..=100).contains(&dy)
    {
        return Err(AppError::new(ErrorCode::InvalidInput, "无效的移动请求"));
    }
    let window = window
        .app_handle()
        .get_webview_window("result")
        .ok_or_else(|| AppError::new(ErrorCode::Internal, "结果窗口未打开"))?;
    let position = window
        .outer_position()
        .map_err(|_| AppError::new(ErrorCode::Internal, "无法读取窗口位置"))?;
    window
        .set_position(tauri::PhysicalPosition::new(
            position.x + dx,
            position.y + dy,
        ))
        .map_err(|_| AppError::new(ErrorCode::Internal, "无法移动结果窗口"))
}

fn save_configuration(app: &AppHandle, config: Config) -> Result<()> {
    config.validate()?;
    let state = app.state::<AppState>();
    let old = lock(&state.config).clone();
    if !cfg!(feature = "e2e")
        && let Err(error) = shortcuts::apply(app, &config.hotkeys)
    {
        let _ = shortcuts::apply(app, &old.hotkeys);
        return Err(error);
    }
    if !cfg!(feature = "e2e") && config.autostart != old.autostart {
        let result = if config.autostart {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
        };
        if result.is_err() {
            let _ = shortcuts::apply(app, &old.hotkeys);
            return Err(AppError::new(
                ErrorCode::Configuration,
                "无法修改系统自动启动设置",
            ));
        }
    }
    if let Err(error) = config.save(&state.config_path) {
        let _ = shortcuts::apply(app, &old.hotkeys);
        if !cfg!(feature = "e2e") && old.autostart {
            let _ = app.autolaunch().enable();
        } else if !cfg!(feature = "e2e") {
            let _ = app.autolaunch().disable();
        }
        return Err(error);
    }
    let translation_changed = config.provider != old.provider
        || config.providers != old.providers
        || config.source != old.source
        || config.target != old.target
        || config.chinese_target != old.chinese_target
        || config.ocr != old.ocr;
    {
        let mut sessions = lock(&state.sessions);
        if translation_changed {
            state.jobs.cancel("text");
            state.jobs.cancel("visual");
            state.jobs.cancel_prefix("block:");
            sessions.text.revision += 1;
            sessions.visual.revision += 1;
            sessions.text.job.stage = JobStage::Cancelled;
            sessions.text.job.error = None;
            sessions.visual.job.stage = JobStage::Cancelled;
            sessions.visual.job.error = None;
        }
        *lock(&state.config) = config.clone();
    }
    if translation_changed {
        state.publish_text(app);
        state.publish_visual(app);
    }
    lock(&state.dirty_windows).remove("main");
    if lock(&state.theme_preview).commit(old.appearance.theme, config.appearance.theme) {
        windows::apply_theme(app, config.appearance.theme);
    }
    let mut broadcast = config;
    broadcast.appearance.theme = lock(&state.theme_preview).effective(broadcast.appearance.theme);
    let _ = app.emit("config-changed", broadcast);
    Ok(())
}

#[tauri::command]
pub async fn save_config(app: AppHandle, window: WebviewWindow, config: Config) -> Result<Config> {
    main_only(&window)?;
    save_configuration(&app, config.clone())?;
    Ok(config)
}

#[tauri::command]
pub async fn set_dirty(app: AppHandle, window: WebviewWindow, dirty: bool) -> Result<()> {
    let state = app.state::<AppState>();
    if dirty {
        lock(&state.dirty_windows).insert(window.label().into());
    } else {
        lock(&state.dirty_windows).remove(window.label());
    }
    Ok(())
}

fn valid_secret_name(name: &str) -> Result<()> {
    if !matches!(name, "azure_ocr" | "custom_ocr")
        && !ProviderId::ALL.iter().any(|id| id.key() == name)
    {
        return Err(AppError::new(ErrorCode::InvalidInput, "无效的凭据名称"));
    }
    Ok(())
}

#[tauri::command]
pub async fn save_secret(
    app: AppHandle,
    window: WebviewWindow,
    name: String,
    secret: String,
    persistent: bool,
) -> Result<bool> {
    main_only(&window)?;
    valid_secret_name(&name)?;
    if secret.len() > 8192 {
        return Err(AppError::new(ErrorCode::InvalidInput, "密钥过长"));
    }
    app.state::<AppState>()
        .vault
        .set(&name, secret.trim(), persistent)
}

#[tauri::command]
pub async fn read_secret(
    app: AppHandle,
    window: WebviewWindow,
    name: String,
) -> Result<Option<String>> {
    main_only(&window)?;
    valid_secret_name(&name)?;
    app.state::<AppState>().vault.get(&name)
}

#[tauri::command]
pub async fn delete_secret(app: AppHandle, window: WebviewWindow, name: String) -> Result<()> {
    main_only(&window)?;
    valid_secret_name(&name)?;
    app.state::<AppState>().vault.delete(&name)
}

#[tauri::command]
pub async fn credential_status(app: AppHandle) -> Result<BTreeMap<String, bool>> {
    Ok(app.state::<AppState>().bootstrap().credentials)
}

#[tauri::command]
pub async fn test_provider(
    app: AppHandle,
    window: WebviewWindow,
    provider: ProviderId,
) -> Result<String> {
    main_only(&window)?;
    let state = app.state::<AppState>();
    let config = lock(&state.config).clone();
    let backend = HttpTranslator::new(
        provider,
        config.providers.get(&provider).cloned().unwrap_or_default(),
        state
            .vault
            .get(&provider.key())
            .ok()
            .flatten()
            .unwrap_or_default(),
    )?;
    let output = backend
        .translate(
            screentrans_core::TranslationRequest {
                source: "en".into(),
                target: "zh-Hans".into(),
                items: vec![screentrans_core::TranslationItem {
                    id: "test".into(),
                    text: "Hello".into(),
                }],
            },
            CancellationToken::new(),
        )
        .await?;
    Ok(output[0].text.clone())
}

#[tauri::command]
pub async fn list_models(
    app: AppHandle,
    window: WebviewWindow,
    provider: ProviderId,
) -> Result<Vec<String>> {
    main_only(&window)?;
    let state = app.state::<AppState>();
    let config = lock(&state.config).clone();
    HttpTranslator::new(
        provider,
        config.providers.get(&provider).cloned().unwrap_or_default(),
        state
            .vault
            .get(&provider.key())
            .ok()
            .flatten()
            .unwrap_or_default(),
    )?
    .list_models(CancellationToken::new())
    .await
}

#[tauri::command]
pub async fn import_legacy_config(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<MigrationReport> {
    main_only(&window)?;
    let state = app.state::<AppState>();
    let path = state
        .legacy_path
        .as_ref()
        .ok_or_else(|| AppError::new(ErrorCode::Configuration, "未找到旧版配置"))?;
    if std::fs::metadata(path)
        .map_err(|_| AppError::storage())?
        .len()
        > 1_048_576
    {
        return Err(AppError::new(ErrorCode::Configuration, "旧配置文件过大"));
    }
    let import = import_legacy(&std::fs::read(path).map_err(|_| AppError::storage())?)?;
    let mut report = MigrationReport {
        credentials_imported: 0,
        warnings: import.warnings,
    };
    for (name, value) in import.secrets {
        match screentrans_platform::credentials::decode_legacy(&value) {
            Ok(secret) if !secret.is_empty() => {
                let durable = state.vault.set(&name, &secret, true)?;
                report.credentials_imported += 1;
                if !durable {
                    report
                        .warnings
                        .push(format!("{name} 密钥仅在本次会话中使用"));
                }
            }
            _ => report
                .warnings
                .push(format!("{name} 密钥无法解密，请重新填写")),
        }
    }
    save_configuration(&app, import.config)?;
    Ok(report)
}

#[tauri::command]
pub async fn open_image(app: AppHandle, fixture_path: Option<String>) -> Result<()> {
    // File-picker bypass exists only in the isolated test build.
    let fixture = if cfg!(feature = "e2e") {
        fixture_path.map(std::path::PathBuf::from)
    } else {
        None
    };
    let path = if let Some(path) = fixture {
        path
    } else {
        let selection = app
            .dialog()
            .file()
            .add_filter("图片", &["png", "jpg", "jpeg", "webp"])
            .blocking_pick_file();
        let Some(selection) = selection else {
            return Ok(());
        };
        selection
            .into_path()
            .map_err(|_| AppError::new(ErrorCode::InvalidInput, "无法读取所选文件"))?
    };
    let frame = tauri::async_runtime::spawn_blocking(move || {
        if std::fs::metadata(&path)
            .map_err(|_| AppError::storage())?
            .len()
            > 64 * 1024 * 1024
        {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "图片文件超过 64MB，请缩小后再打开",
            ));
        }
        let mut reader = image::ImageReader::open(path)
            .map_err(|_| AppError::storage())?
            .with_guessed_format()
            .map_err(|_| AppError::storage())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16000);
        limits.max_image_height = Some(16000);
        limits.max_alloc = Some(160 * 1024 * 1024);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|_| AppError::new(ErrorCode::InvalidInput, "图片格式不支持或图像尺寸过大"))?
            .to_rgba8();
        Ok::<_, AppError>(CaptureFrame {
            image,
            screen_bounds: None,
            scale_factor: 1.0,
        })
    })
    .await
    .map_err(|_| AppError::new(ErrorCode::Internal, "图片读取线程异常"))??;
    let metadata = lock(&app.state::<AppState>().frames).insert(frame, &[])?;
    pipeline::start_visual(app, metadata, true);
    Ok(())
}

#[tauri::command]
pub async fn configure_portal_shortcuts(app: AppHandle, window: WebviewWindow) -> Result<()> {
    main_only(&window)?;
    if cfg!(feature = "e2e") || !screentrans_platform::is_wayland() {
        return Err(AppError::new(
            ErrorCode::Unsupported,
            "当前环境无需配置 Wayland 快捷键",
        ));
    }
    let mut config = lock(&app.state::<AppState>().config).clone();
    config.wayland_shortcuts = true;
    let keys = config.hotkeys.clone();
    save_configuration(&app, config)?;
    shortcuts::configure_portal(app, keys);
    Ok(())
}

#[tauri::command]
pub async fn open_help(app: AppHandle, topic: String) -> Result<()> {
    let url = match topic.as_str() {
        "permissions" if cfg!(target_os = "macos") => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
        }
        "permissions" if cfg!(target_os = "linux") => {
            "https://flatpak.github.io/xdg-desktop-portal/docs/"
        }
        "permissions" => "https://github.com/reevebyte/ScreenTranslate#文字识别",
        "ocr" if cfg!(windows) => "ms-settings:regionlanguage",
        "ocr" => "https://tesseract-ocr.github.io/tessdoc/Installation.html",
        "issues" => "https://github.com/reevebyte/ScreenTranslate/issues",
        "releases" => "https://github.com/reevebyte/ScreenTranslate/releases",
        "repository" => "https://github.com/reevebyte/ScreenTranslate",
        _ => return Err(AppError::new(ErrorCode::InvalidInput, "未知帮助主题")),
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| AppError::new(ErrorCode::Internal, "无法打开系统设置或帮助页面"))
}

#[tauri::command]
pub async fn quit(app: AppHandle) {
    app.state::<AppState>().jobs.cancel_all();
    app.exit(0);
}
