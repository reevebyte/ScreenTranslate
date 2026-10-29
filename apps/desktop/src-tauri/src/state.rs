use crate::cache::FrameCache;
use screentrans_core::{
    Bootstrap, MonitorInfo, PlatformCapabilities, TranslationSession,
    config::{Config, ProviderId},
    jobs::JobRegistry,
    ports::CaptureBackend,
};
use screentrans_platform::{capture::SystemCapture, credentials::CredentialVault};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::{Mutex, MutexGuard},
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
    #[cfg(feature = "e2e")]
    _test_directory: tempfile::TempDir,
    pub config: Mutex<Config>,
    pub theme_preview: Mutex<crate::theme_preview::ThemePreview>,
    pub config_path: PathBuf,
    pub legacy_path: Option<PathBuf>,
    pub sessions: Mutex<Sessions>,
    pub jobs: JobRegistry,
    pub frames: Mutex<FrameCache>,
    pub selections: Mutex<HashMap<String, String>>,
    pub capture_layout: Mutex<Vec<MonitorInfo>>,
    pub return_windows: Mutex<Vec<String>>,
    pub result_layout: Mutex<crate::windows::ResultLayout>,
    pub result_toolbar: Mutex<crate::result_toolbar::ToolbarState>,
    pub result_display: Mutex<crate::result_toolbar::ResultDisplay>,
    pub result_frame_id: Mutex<String>,
    pub result_resize: Mutex<Option<crate::result_resize::ResizeGesture>>,
    pub window_lifecycle: Mutex<crate::windows::WindowLifecycle>,
    pub vault: CredentialVault,
    pub capabilities: Mutex<PlatformCapabilities>,
    pub warnings: Mutex<Vec<String>>,
    pub dirty_windows: Mutex<HashSet<String>>,
    #[cfg(target_os = "linux")]
    pub portal_stop: Mutex<CancellationToken>,
}

impl AppState {
    pub fn new(app: &AppHandle) -> Self {
        let mut warnings = Vec::new();
        #[cfg(not(feature = "e2e"))]
        let directory = app.path().app_config_dir().unwrap_or_else(|_| {
            warnings.push("无法找到系统配置目录，本次运行的设置无法持久保存".into());
            std::env::temp_dir().join("screentranslate-v2")
        });
        #[cfg(feature = "e2e")]
        let test_directory =
            tempfile::tempdir().expect("Could not create isolated E2E configuration");
        #[cfg(feature = "e2e")]
        let directory = test_directory.path().to_path_buf();
        #[cfg(feature = "e2e")]
        let _ = app;
        let config_path = directory.join("config.json");
        let config = Config::load(&config_path).unwrap_or_else(|error| {
            warnings.push(error.message);
            Config::default()
        });
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
            #[cfg(feature = "e2e")]
            _test_directory: test_directory,
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
            capabilities: Mutex::new(screentrans_platform::capabilities()),
            warnings: Mutex::new(warnings),
            dirty_windows: Mutex::new(HashSet::new()),
            #[cfg(target_os = "linux")]
            portal_stop: Mutex::new(CancellationToken::new()),
        }
    }

    pub fn bootstrap(&self) -> Bootstrap {
        let credentials = ProviderId::ALL
            .into_iter()
            .map(|provider| {
                let name = provider.key();
                let present = self
                    .vault
                    .get(&name)
                    .ok()
                    .flatten()
                    .is_some_and(|v| !v.is_empty());
                (name, present)
            })
            .chain(std::iter::once((
                "azure_ocr".into(),
                self.vault.get("azure_ocr").ok().flatten().is_some(),
            )))
            .chain(std::iter::once((
                "custom_ocr".into(),
                self.vault.get("custom_ocr").ok().flatten().is_some(),
            )))
            .collect::<BTreeMap<_, _>>();
        let mut config = lock(&self.config).clone();
        config.appearance.theme = lock(&self.theme_preview).effective(config.appearance.theme);
        let sessions = lock(&self.sessions);
        Bootstrap {
            version: env!("CARGO_PKG_VERSION").into(),
            initial_route: lock(&self.route).clone(),
            config,
            capabilities: lock(&self.capabilities).clone(),
            text: sessions.text.clone(),
            visual: sessions.visual.clone(),
            credentials,
            warnings: lock(&self.warnings).clone(),
            monitors: SystemCapture.monitors().unwrap_or_default(),
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
