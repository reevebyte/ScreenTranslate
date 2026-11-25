use crate::cache::FrameCache;
use screentrans_core::{
    Bootstrap, MonitorInfo, PlatformCapabilities, TranslationSession,
    config::{Config, ProviderId},
    jobs::JobRegistry,
    ports::CaptureBackend,
};
use screentrans_platform::{
    capture::SystemCapture, credentials::CredentialVault, rapid_ocr::RapidOcrManager,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};
#[cfg(not(feature = "e2e"))]
use tauri::Manager;
use tauri::{AppHandle, Emitter};
#[cfg(target_os = "linux")]
use tokio_util::sync::CancellationToken;

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

#[derive(Default)]
pub struct Sessions {
    pub text: TranslationSession,
    pub visual: TranslationSession,
    pub clients: HashMap<String, u32>,
}

pub struct AppState {
    pub route: Mutex<String>,
    _temp_directory: Option<tempfile::TempDir>,
    pub config: Mutex<Config>,
    pub theme_preview: Mutex<crate::theme_preview::ThemePreview>,
    pub config_path: PathBuf,
    pub legacy_path: Option<PathBuf>,
    pub sessions: Mutex<Sessions>,
    pub jobs: JobRegistry,
    pub frames: Mutex<FrameCache>,
    pub selections: Mutex<HashMap<String, String>>,
    pub capture_layout: Mutex<Vec<MonitorInfo>>,
    pub capture_mode: Mutex<crate::windows::CaptureMode>,
    pub return_windows: Mutex<Vec<String>>,
    pub result_layout: Mutex<crate::windows::ResultLayout>,
    pub result_toolbar: Mutex<crate::result_toolbar::ToolbarState>,
    pub result_display: Mutex<crate::result_toolbar::ResultDisplay>,
    pub result_frame_id: Mutex<String>,
    pub result_resize: Mutex<Option<crate::result_resize::ResizeGesture>>,
    pub window_lifecycle: Mutex<crate::windows::WindowLifecycle>,
    pub vault: CredentialVault,
    pub rapid_ocr: Arc<RapidOcrManager>,
    pub capabilities: Mutex<PlatformCapabilities>,
    pub warnings: Mutex<Vec<String>>,
    pub dirty_windows: Mutex<HashSet<String>>,
    #[cfg(target_os = "linux")]
    pub portal_stop: Mutex<CancellationToken>,
    #[cfg(target_os = "linux")]
    pub hyprland_shortcuts: Mutex<Vec<String>>,
}

impl AppState {
    pub fn new(app: &AppHandle) -> Self {
        let mut warnings = Vec::new();
        let mut temp_directory: Option<tempfile::TempDir> = None;
        #[cfg(not(feature = "e2e"))]
        let directory = match app.path().app_config_dir() {
            Ok(dir) => dir,
            Err(_) => {
                warnings.push("无法找到系统配置目录，本次运行的设置无法持久保存".into());
                match tempfile::tempdir() {
                    Ok(dir) => {
                        let path = dir.path().to_path_buf();
                        temp_directory = Some(dir);
                        path
                    }
                    Err(_) => std::env::temp_dir()
                        .join(format!("screentranslate-v2-{}", std::process::id())),
                }
            }
        };
        #[cfg(feature = "e2e")]
        let directory = {
            let dir = tempfile::tempdir().expect("Could not create isolated E2E configuration");
            let path = dir.path().to_path_buf();
            temp_directory = Some(dir);
            let _ = app;
            path
        };
        let config_path = directory.join("config.json");
        #[cfg(not(feature = "e2e"))]
        let rapid_ocr_dir = app
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| directory.clone())
            .join("ocr")
            .join("rapidocr");
        #[cfg(feature = "e2e")]
        let rapid_ocr_dir = directory.join("ocr").join("rapidocr");
        let config = Config::load(&config_path).unwrap_or_else(|error| {
            warnings.push(error.message);
            // Defaults will be saved over this file on the next settings change.
            if let Some(backup) = Config::back_up_unreadable(&config_path) {
                let name = backup.file_name().unwrap_or_default().to_string_lossy();
                warnings.push(format!("原配置已备份为 {name}，本次使用默认设置"));
            }
            Config::default()
        });
        let rapid_ocr = RapidOcrManager::new(rapid_ocr_dir);
        rapid_ocr.set_idle_unload_secs(config.ocr.rapid.idle_unload_secs);
        let legacy_path = if cfg!(windows) && !cfg!(feature = "e2e") {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .map(|directory| directory.join("ScreenTranslate/config.json"))
                .filter(|path| path.is_file())
        } else {
            None
        };
        let text = TranslationSession {
            source: config.source.clone(),
            target: config.target.clone(),
            ..TranslationSession::default()
        };
        let visual = TranslationSession {
            id: "visual".into(),
            ..TranslationSession::default()
        };
        Self {
            route: Mutex::new("translate".into()),
            _temp_directory: temp_directory,
            config: Mutex::new(config),
            theme_preview: Mutex::new(crate::theme_preview::ThemePreview::default()),
            config_path,
            legacy_path,
            sessions: Mutex::new(Sessions {
                text,
                visual,
                clients: HashMap::new(),
            }),
            jobs: JobRegistry::default(),
            frames: Mutex::new(FrameCache::default()),
            selections: Mutex::new(HashMap::new()),
            capture_layout: Mutex::new(Vec::new()),
            capture_mode: Mutex::new(crate::windows::CaptureMode::default()),
            return_windows: Mutex::new(Vec::new()),
            result_layout: Mutex::new(crate::windows::ResultLayout::default()),
            result_toolbar: Mutex::new(crate::result_toolbar::ToolbarState::default()),
            result_display: Mutex::new(crate::result_toolbar::ResultDisplay::default()),
            result_frame_id: Mutex::new(String::new()),
            result_resize: Mutex::new(None),
            window_lifecycle: Mutex::new(crate::windows::WindowLifecycle::default()),
            vault: if cfg!(feature = "e2e") {
                CredentialVault::session_only()
            } else {
                CredentialVault::default()
            },
            rapid_ocr,
            capabilities: Mutex::new(screentrans_platform::capabilities()),
            warnings: Mutex::new(warnings),
            dirty_windows: Mutex::new(HashSet::new()),
            #[cfg(target_os = "linux")]
            portal_stop: Mutex::new(CancellationToken::new()),
            #[cfg(target_os = "linux")]
            hyprland_shortcuts: Mutex::new(Vec::new()),
        }
    }

    pub fn bootstrap(&self, consume_warnings: bool) -> Bootstrap {
        let mut persistent_available = true;
        let mut has_secret = |name: &str| {
            if let Some(value) = self.vault.get_session(name) {
                return !value.is_empty();
            }
            if !persistent_available {
                return false;
            }
            match self.vault.get(name) {
                Ok(Some(value)) => !value.is_empty(),
                Ok(None) => false,
                Err(_) => {
                    persistent_available = false;
                    false
                }
            }
        };
        let mut credentials = BTreeMap::new();
        for provider in ProviderId::ALL {
            let name = provider.key();
            let present = has_secret(&name);
            credentials.insert(name, present);
        }
        credentials.insert("azure_ocr".into(), has_secret("azure_ocr"));
        credentials.insert("custom_ocr".into(), has_secret("custom_ocr"));
        let mut config = lock(&self.config).clone();
        config.appearance.theme = lock(&self.theme_preview).effective(config.appearance.theme);
        let (text, visual) = {
            let sessions = lock(&self.sessions);
            (sessions.text.clone(), sessions.visual.clone())
        };
        let warnings = if consume_warnings {
            std::mem::take(&mut *lock(&self.warnings))
        } else {
            Vec::new()
        };
        let monitors = SystemCapture.monitors().unwrap_or_default();
        Bootstrap {
            version: env!("CARGO_PKG_VERSION").into(),
            initial_route: lock(&self.route).clone(),
            config,
            capabilities: lock(&self.capabilities).clone(),
            text,
            visual,
            credentials,
            warnings,
            monitors,
            legacy_available: self.legacy_path.is_some(),
            updater_ready: !crate::updater::public_key().is_empty(),
        }
    }

    pub fn publish_text(&self, app: &AppHandle) {
        let _ = app.emit("session-changed", lock(&self.sessions).text.clone());
    }
    pub fn publish_visual(&self, app: &AppHandle) {
        let _ = app.emit("session-changed", lock(&self.sessions).visual.clone());
    }
    pub fn warning(&self, app: &AppHandle, message: String) {
        let mut warnings = lock(&self.warnings);
        if !warnings.contains(&message) {
            warnings.push(message.clone());
        }
        let _ = app.emit("app-warning", message);
    }
}
