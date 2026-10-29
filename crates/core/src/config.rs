use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Write,
    path::Path,
};
use ts_rs::TS;

use crate::{AppError, ErrorCode, Result};

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    GoogleFree,
    Openai,
    Microsoft,
    Deepl,
    Google,
    Anthropic,
    Nvidia,
    BingFree,
    MicrosoftFree,
    TencentFree,
    YandexFree,
    IcibaFree,
}

impl ProviderId {
    pub const ALL: [Self; 12] = [
        Self::GoogleFree,
        Self::Openai,
        Self::Microsoft,
        Self::Deepl,
        Self::Google,
        Self::Anthropic,
        Self::Nvidia,
        Self::BingFree,
        Self::MicrosoftFree,
        Self::TencentFree,
        Self::YandexFree,
        Self::IcibaFree,
    ];
    pub fn key(self) -> String {
        serde_json::to_value(self).unwrap().as_str().unwrap().into()
    }
    pub fn needs_key(self) -> bool {
        matches!(
            self,
            Self::Microsoft | Self::Deepl | Self::Google | Self::Anthropic | Self::Nvidia
        )
    }
    pub fn unofficial(self) -> bool {
        matches!(
            self,
            Self::GoogleFree
                | Self::BingFree
                | Self::MicrosoftFree
                | Self::TencentFree
                | Self::YandexFree
                | Self::IcibaFree
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderConfig {
    pub base_url: String,
    pub model: String,
    pub region: String,
    pub free_plan: bool,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            model: String::new(),
            region: String::new(),
            free_plan: true,
        }
    }
}

pub fn provider_defaults(provider: ProviderId) -> ProviderConfig {
    ProviderConfig {
        base_url: match provider {
            ProviderId::Openai => "https://api.openai.com/v1",
            ProviderId::Microsoft => "https://api.cognitive.microsofttranslator.com",
            ProviderId::Deepl => "https://api-free.deepl.com/v2",
            ProviderId::Google => "https://translation.googleapis.com/language/translate/v2",
            ProviderId::Anthropic => "https://api.anthropic.com/v1",
            ProviderId::Nvidia => "https://integrate.api.nvidia.com/v1",
            _ => "",
        }
        .into(),
        ..ProviderConfig::default()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum OcrEngine {
    Youdao,
    System,
    Azure,
    Custom,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum OcrProtocol {
    #[default]
    Openai,
    HttpJson,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomOcrConfig {
    pub protocol: OcrProtocol,
    pub endpoint: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct OcrConfig {
    /// Apply the requested cloud default once, preserving later explicit choices.
    #[serde(default)]
    pub defaults_version: u8,
    pub engine: OcrEngine,
    pub languages: Vec<String>,
    pub azure_endpoint: String,
    pub upscale: bool,
    pub custom: CustomOcrConfig,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            defaults_version: 1,
            engine: OcrEngine::Youdao,
            languages: vec!["zh-Hans-CN".into(), "en-US".into()],
            azure_endpoint: String::new(),
            upscale: true,
            custom: CustomOcrConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct Hotkeys {
    pub capture: String,
    pub toggle: String,
    pub quick: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            capture: "Ctrl+Alt+Q".into(),
            toggle: "Ctrl+Alt+W".into(),
            quick: "Ctrl+Alt+Space".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct Appearance {
    pub theme: Theme,
    pub accent: String,
    pub font_scale: f64,
    pub auto_copy: bool,
    pub close_to_tray: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            accent: "#18181b".into(),
            font_scale: 1.0,
            auto_copy: false,
            close_to_tray: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub schema_version: u32,
    pub onboarding_complete: bool,
    pub auto_update: bool,
    pub provider: ProviderId,
    pub providers: BTreeMap<ProviderId, ProviderConfig>,
    pub source: String,
    pub target: String,
    pub chinese_target: String,
    pub auto_translate: bool,
    pub autostart: bool,
    pub wayland_shortcuts: bool,
    pub ocr: OcrConfig,
    pub hotkeys: Hotkeys,
    pub appearance: Appearance,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            onboarding_complete: false,
            auto_update: true,
            provider: ProviderId::GoogleFree,
            providers: ProviderId::ALL
                .into_iter()
                .map(|id| (id, provider_defaults(id)))
                .collect(),
            source: "auto".into(),
            target: "auto".into(),
            chinese_target: "en".into(),
            auto_translate: true,
            autostart: false,
            wayland_shortcuts: false,
            ocr: OcrConfig::default(),
            hotkeys: Hotkeys::default(),
            appearance: Appearance::default(),
        }
    }
}

pub const LANGUAGES: [&str; 10] = [
    "auto", "zh-Hans", "zh-Hant", "en", "ja", "ko", "fr", "de", "es", "ru",
];

impl Config {
    pub fn validate(&self) -> Result<()> {
        let invalid =
            |message: &str| AppError::new(ErrorCode::Configuration, message).action("settings");
        if self.schema_version != SCHEMA_VERSION {
            return Err(invalid("配置版本不受支持，请使用匹配版本的程序"));
        }
        if !LANGUAGES.contains(&self.source.as_str())
            || !LANGUAGES.contains(&self.target.as_str())
            || !LANGUAGES[2..].contains(&self.chinese_target.as_str())
        {
            return Err(invalid("语言配置无效"));
        }
        if !self.appearance.font_scale.is_finite()
            || !(0.8..=1.5).contains(&self.appearance.font_scale)
        {
            return Err(invalid("界面缩放必须介于 80% 和 150%"));
        }
        if self.appearance.accent.len() != 7
            || !self.appearance.accent.starts_with('#')
            || !self.appearance.accent[1..]
                .chars()
                .all(|c| c.is_ascii_hexdigit())
        {
            return Err(invalid("强调色必须是六位十六进制颜色"));
        }
        let keys = [
            &self.hotkeys.capture,
            &self.hotkeys.toggle,
            &self.hotkeys.quick,
        ];
        for (i, key) in keys.iter().enumerate() {
            if !key.is_empty()
                && keys[..i]
                    .iter()
                    .any(|prior| prior.eq_ignore_ascii_case(key))
            {
                return Err(invalid("三个全局快捷键不能重复"));
            }
        }
        if self.ocr.languages.is_empty() || self.ocr.languages.len() > 16 {
            return Err(invalid("请至少选择一种 OCR 语言"));
        }
        for (id, provider) in &self.providers {
            if !provider.base_url.is_empty() {
                validate_api_url(&provider.base_url)?;
            }
            if provider.model.len() > 200 || provider.region.len() > 100 {
                return Err(invalid("服务配置过长"));
            }
            if !ProviderId::ALL.contains(id) {
                return Err(invalid("翻译服务无效"));
            }
        }
        if !self.ocr.azure_endpoint.is_empty() {
            validate_api_url(&self.ocr.azure_endpoint)?;
        }
        if !self.ocr.custom.endpoint.is_empty() {
            validate_api_url(&self.ocr.custom.endpoint)?;
        }
        if self.ocr.custom.model.len() > 200 {
            return Err(invalid("OCR 模型名称过长"));
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = fs::read(path).map_err(|_| AppError::storage())?;
        if bytes.len() > 1_048_576 {
            return Err(AppError::new(ErrorCode::Configuration, "配置文件过大"));
        }
        let mut config: Self = serde_json::from_slice(&bytes).map_err(|_| {
            AppError::new(ErrorCode::Configuration, "配置文件格式损坏；原文件已保留")
                .action("settings")
        })?;
        for id in ProviderId::ALL {
            config
                .providers
                .entry(id)
                .or_insert_with(|| provider_defaults(id));
        }
        config.validate()?;
        if config.ocr.defaults_version == 0 {
            config.ocr.engine = OcrEngine::Youdao;
            config.ocr.defaults_version = 1;
            config.save(path)?;
        }
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let parent = path.parent().ok_or_else(AppError::storage)?;
        fs::create_dir_all(parent).map_err(|_| AppError::storage())?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| AppError::storage())?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| AppError::storage())?;
        temporary
            .write_all(&bytes)
            .map_err(|_| AppError::storage())?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| AppError::storage())?;
        temporary.persist(path).map_err(|_| AppError::storage())?;
        Ok(())
    }
}

pub fn validate_api_url(value: &str) -> Result<url::Url> {
    let invalid = || {
        AppError::new(
            ErrorCode::Configuration,
            "服务地址必须使用 HTTPS；本机服务可以使用 HTTP",
        )
        .action("providers")
    };
    let url = url::Url::parse(value).map_err(|_| invalid())?;
    let local = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !url.query().unwrap_or_default().is_empty()
        || !(url.scheme() == "https" || (url.scheme() == "http" && local))
    {
        return Err(invalid());
    }
    Ok(url)
}

pub struct LegacyImport {
    pub config: Config,
    /// Protected values are decoded by the platform adapter, never returned to the UI.
    pub secrets: HashMap<String, String>,
    pub warnings: Vec<String>,
}

pub fn import_legacy(bytes: &[u8]) -> Result<LegacyImport> {
    let root: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| AppError::new(ErrorCode::Configuration, "旧版配置不是有效的 JSON"))?;
    if !root.is_object() {
        return Err(AppError::new(ErrorCode::Configuration, "旧版配置结构无效"));
    }
    let mut config = Config::default();
    let mut secrets = HashMap::new();
    let mut warnings = Vec::new();
    let string = |pointer: &str| {
        root.pointer(pointer)
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    };
    if let Some(provider) = root
        .pointer("/translator/provider")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        config.provider = provider;
    }
    for id in ProviderId::ALL {
        let key = id.key();
        if let Some(old) = root.pointer(&format!("/translator/{key}")) {
            let options = config.providers.get_mut(&id).unwrap();
            if let Some(value) = old
                .get("base_url")
                .or(old.get("endpoint"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                options.base_url = if id == ProviderId::Anthropic {
                    format!("{}/v1", value.trim_end_matches('/'))
                } else {
                    value.into()
                };
            }
            if let Some(value) = old.get("model").and_then(|v| v.as_str()) {
                options.model = value.into();
            }
            if let Some(value) = old.get("region").and_then(|v| v.as_str()) {
                options.region = value.into();
            }
            if let Some(value) = old.get("free_plan").and_then(|v| v.as_bool()) {
                options.free_plan = value;
                if id == ProviderId::Deepl
                    && old.get("base_url").is_none()
                    && old.get("endpoint").is_none()
                {
                    options.base_url = if value {
                        "https://api-free.deepl.com/v2"
                    } else {
                        "https://api.deepl.com/v2"
                    }
                    .into();
                }
            }
            if let Some(value) = old
                .get("key")
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
            {
                secrets.insert(key, value.into());
            }
        }
    }
    if let Some(v) = string("/hotkey") {
        config.hotkeys.capture = v;
    }
    if let Some(v) = string("/hotkey_toggle") {
        config.hotkeys.toggle = v;
    }
    if let Some(v) = string("/hotkey_text_translate") {
        config.hotkeys.quick = v;
    }
    if let Some(v) = string("/lang/zh_target") {
        config.chinese_target = v;
    }
    if let Some(v) = root.get("autostart").and_then(|v| v.as_bool()) {
        config.autostart = v;
    }
    if let Some(v) = root
        .pointer("/appearance/auto_copy")
        .and_then(|v| v.as_bool())
    {
        config.appearance.auto_copy = v;
    }
    if let Some(value) = string("/appearance/accent") {
        if value.len() == 7
            && value.starts_with('#')
            && value[1..].chars().all(|c| c.is_ascii_hexdigit())
        {
            config.appearance.accent = value;
        } else {
            warnings.push("旧版强调色无效，已使用新版默认颜色".into());
        }
    }
    if let Some(v) = root.pointer("/ocr/upscale").and_then(|v| v.as_bool()) {
        config.ocr.upscale = v;
    }
    if let Some(v) = root
        .pointer("/ocr/languages")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        config.ocr.languages = v;
    }
    config.ocr.engine = match string("/ocr/engine").as_deref() {
        Some("azure_vision") => OcrEngine::Azure,
        Some("windows") | Some("windows_ocr") | Some("system") => OcrEngine::System,
        _ => OcrEngine::Youdao,
    };
    if let Some(v) = string("/ocr/azure_vision/endpoint") {
        config.ocr.azure_endpoint = v;
    }
    if let Some(v) = string("/ocr/azure_vision/key").filter(|v| !v.is_empty()) {
        secrets.insert("azure_ocr".into(), v);
    }
    config.validate()?;
    Ok(LegacyImport {
        config,
        secrets,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cloud_default_is_applied_once_and_later_local_choice_survives() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let mut old = serde_json::to_value(Config::default()).unwrap();
        old["ocr"]["engine"] = serde_json::json!("system");
        old["ocr"]
            .as_object_mut()
            .unwrap()
            .remove("defaultsVersion");
        fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        let mut updated = Config::load(&path).unwrap();
        assert_eq!(updated.ocr.engine, OcrEngine::Youdao);
        updated.ocr.engine = OcrEngine::System;
        updated.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap().ocr.engine, OcrEngine::System);
    }

    #[test]
    fn old_settings_enable_updates_without_reopening_onboarding() {
        let mut value = serde_json::to_value(Config::default()).unwrap();
        value.as_object_mut().unwrap().remove("autoUpdate");
        value["onboardingComplete"] = serde_json::Value::Bool(true);
        let mut config: Config = serde_json::from_value(value).unwrap();
        assert!(config.auto_update);
        assert!(config.onboarding_complete);
        config.auto_update = false;
        let saved = serde_json::to_vec(&config).unwrap();
        assert!(
            !serde_json::from_slice::<Config>(&saved)
                .unwrap()
                .auto_update
        );
    }

    #[test]
    fn roundtrip_and_future_version_protection() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let config = Config::default();
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap().schema_version, 2);
        let future = Config {
            schema_version: 999,
            ..config
        };
        assert!(future.save(&path).is_err());
        assert_eq!(Config::load(&path).unwrap().schema_version, 2);
    }

    #[test]
    fn legacy_migration_keeps_protected_secrets_separate() {
        let migration = import_legacy(br#"{"translator":{"provider":"openai","openai":{"base_url":"http://localhost:11434/v1","model":"local","key":"dpapi:v1:fixture"}},"lang":{"zh_target":"ja"}}"#).unwrap();
        assert_eq!(migration.config.provider, ProviderId::Openai);
        assert_eq!(migration.config.chinese_target, "ja");
        assert_eq!(migration.secrets["openai"], "dpapi:v1:fixture");
        assert!(
            !serde_json::to_string(&migration.config)
                .unwrap()
                .contains("fixture")
        );
    }

    #[test]
    fn disallows_remote_plain_http_and_embedded_credentials() {
        for url in [
            "http://example.com",
            "https://key@example.com",
            "https://example.com?key=secret",
        ] {
            assert!(validate_api_url(url).is_err());
        }
        for url in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:8000/v1",
            "http://[::1]:8000/v1",
            "https://example.com/v1",
        ] {
            assert!(validate_api_url(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn legacy_provider_urls_and_brand_color_keep_their_original_meaning() {
        let migration = import_legacy(br##"{"translator":{"provider":"anthropic","anthropic":{"base_url":"https://proxy.example/anthropic","model":"fixture-model"},"deepl":{"free_plan":false}},"appearance":{"accent":"#28C76F","auto_copy":true}}"##).unwrap();
        assert_eq!(
            migration.config.providers[&ProviderId::Anthropic].base_url,
            "https://proxy.example/anthropic/v1"
        );
        assert_eq!(
            migration.config.providers[&ProviderId::Deepl].base_url,
            "https://api.deepl.com/v2"
        );
        assert_eq!(migration.config.appearance.accent, "#28C76F");
        assert!(migration.config.appearance.auto_copy);
    }
}
