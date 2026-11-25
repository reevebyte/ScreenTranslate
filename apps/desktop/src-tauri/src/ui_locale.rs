//! Lightweight localization table for native OS surfaces (system tray menu,
//! Windows caption update pill, and Windows native selection overlay guide).

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiLocale {
    #[default]
    ZhCn = 0,
    ZhTw = 1,
    En = 2,
    Ja = 3,
    Ko = 4,
    Fr = 5,
    De = 6,
    Es = 7,
    Ru = 8,
}

impl UiLocale {
    pub fn from_code(code: &str) -> Self {
        let lower = code.trim().to_ascii_lowercase();
        if lower.is_empty() {
            return Self::ZhCn;
        }
        if lower.starts_with("zh-tw")
            || lower.starts_with("zh-hk")
            || lower.starts_with("zh-mo")
            || lower.starts_with("zh-hant")
        {
            Self::ZhTw
        } else if lower.starts_with("zh") {
            Self::ZhCn
        } else if lower.starts_with("ja") {
            Self::Ja
        } else if lower.starts_with("ko") {
            Self::Ko
        } else if lower.starts_with("fr") {
            Self::Fr
        } else if lower.starts_with("de") {
            Self::De
        } else if lower.starts_with("es") {
            Self::Es
        } else if lower.starts_with("ru") {
            Self::Ru
        } else {
            Self::En
        }
    }

    pub fn resolve(locale: Option<&str>, english: Option<bool>) -> Self {
        if let Some(code) = locale.filter(|s| !s.trim().is_empty()) {
            Self::from_code(code)
        } else if english.unwrap_or(false) {
            Self::En
        } else {
            Self::ZhCn
        }
    }

    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::ZhTw,
            2 => Self::En,
            3 => Self::Ja,
            4 => Self::Ko,
            5 => Self::Fr,
            6 => Self::De,
            7 => Self::Es,
            8 => Self::Ru,
            _ => Self::ZhCn,
        }
    }

    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::ZhTw => "zh-TW",
            Self::En => "en",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Fr => "fr",
            Self::De => "de",
            Self::Es => "es",
            Self::Ru => "ru",
        }
    }

    pub const fn tray_open_main(self) -> &'static str {
        match self {
            Self::ZhCn => "打开主窗口",
            Self::ZhTw => "開啟主視窗",
            Self::En => "Open main window",
            Self::Ja => "メインウィンドウを開く",
            Self::Ko => "메인 창 열기",
            Self::Fr => "Ouvrir la fenêtre",
            Self::De => "Hauptfenster öffnen",
            Self::Es => "Abrir ventana principal",
            Self::Ru => "Открыть главное окно",
        }
    }

    pub const fn tray_capture(self) -> &'static str {
        match self {
            Self::ZhCn => "截图翻译",
            Self::ZhTw => "截圖翻譯",
            Self::En => "Capture and translate",
            Self::Ja => "画面キャプチャ翻訳",
            Self::Ko => "화면 캡처 번역",
            Self::Fr => "Capturer et traduire",
            Self::De => "Bildschirm übersetzen",
            Self::Es => "Capturar y traducir",
            Self::Ru => "Снимок и перевод",
        }
    }

    pub const fn tray_quick(self) -> &'static str {
        match self {
            Self::ZhCn => "快速文字翻译",
            Self::ZhTw => "快速文字翻譯",
            Self::En => "Quick translate",
            Self::Ja => "クイック翻訳",
            Self::Ko => "빠른 텍스트 번역",
            Self::Fr => "Traduction rapide",
            Self::De => "Schnellübersetzung",
            Self::Es => "Traducción rápida",
            Self::Ru => "Быстрый перевод",
        }
    }

    pub const fn tray_quit(self) -> &'static str {
        match self {
            Self::ZhCn => "退出",
            Self::ZhTw => "結束",
            Self::En => "Quit",
            Self::Ja => "終了",
            Self::Ko => "종료",
            Self::Fr => "Quitter",
            Self::De => "Beenden",
            Self::Es => "Salir",
            Self::Ru => "Выход",
        }
    }

    pub fn tray_version(self, target: &str) -> String {
        let prefix = match self {
            Self::ZhCn | Self::ZhTw => "版本",
            Self::En | Self::Fr | Self::De => "Version",
            Self::Ja => "バージョン",
            Self::Ko => "버전",
            Self::Es => "Versión",
            Self::Ru => "Версия",
        };
        format!("{prefix} {target}")
    }

    pub fn tray_new_version(self, target: &str) -> String {
        let prefix = match self {
            Self::ZhCn | Self::ZhTw => "新版本",
            Self::En => "New version",
            Self::Ja => "新しいバージョン",
            Self::Ko => "새 버전",
            Self::Fr => "Nouvelle version",
            Self::De => "Neue Version",
            Self::Es => "Nueva versión",
            Self::Ru => "Новая версия",
        };
        format!("{prefix} v{target}")
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub const fn caption_update(self) -> &'static str {
        match self {
            Self::ZhCn => "下载更新",
            Self::ZhTw => "下載更新",
            Self::En => "Update",
            Self::Ja => "更新",
            Self::Ko => "업데이트",
            Self::Fr => "Mettre à jour",
            Self::De => "Aktualisieren",
            Self::Es => "Actualizar",
            Self::Ru => "Обновить",
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub const fn caption_restart(self) -> &'static str {
        match self {
            Self::ZhCn => "重启更新",
            Self::ZhTw => "重啟更新",
            Self::En => "Restart",
            Self::Ja => "再起動",
            Self::Ko => "재시작",
            Self::Fr => "Redémarrer",
            Self::De => "Neu starten",
            Self::Es => "Reiniciar",
            Self::Ru => "Перезапуск",
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn caption_downloading(self, progress: Option<u32>) -> String {
        if let Some(n) = progress {
            let pct = n.min(99);
            let label = match self {
                Self::ZhCn => "下载",
                Self::ZhTw => "下載",
                Self::En => "Get",
                Self::Ja => "取得",
                Self::Ko => "다운로드",
                Self::Fr => "Téléch.",
                Self::De => "Laden",
                Self::Es => "Bajando",
                Self::Ru => "Загр.",
            };
            format!("{label} {pct}%")
        } else {
            match self {
                Self::ZhCn => "下载中…",
                Self::ZhTw => "下載中…",
                Self::En => "Getting…",
                Self::Ja => "取得中…",
                Self::Ko => "다운로드 중…",
                Self::Fr => "Téléch.…",
                Self::De => "Lädt…",
                Self::Es => "Bajando…",
                Self::Ru => "Загрузка…",
            }
            .into()
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub const fn selection_drag(self) -> &'static str {
        match self {
            Self::ZhCn => "拖动选择区域",
            Self::ZhTw => "拖曳選取區域",
            Self::En => "Drag to select an area",
            Self::Ja => "ドラッグして範囲を選択",
            Self::Ko => "드래그하여 영역 선택",
            Self::Fr => "Glissez pour sélectionner",
            Self::De => "Ziehen zum Auswählen",
            Self::Es => "Arrastra para seleccionar",
            Self::Ru => "Выделите область",
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub const fn selection_recognizing(self) -> &'static str {
        match self {
            Self::ZhCn => "正在识别…",
            Self::ZhTw => "正在辨識…",
            Self::En => "Recognizing…",
            Self::Ja => "認識中…",
            Self::Ko => "인식 중…",
            Self::Fr => "Reconnaissance…",
            Self::De => "Erkennung…",
            Self::Es => "Reconociendo…",
            Self::Ru => "Распознавание…",
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub const fn selection_cancel(self) -> &'static str {
        match self {
            Self::ZhCn => "取消",
            Self::ZhTw => "取消",
            Self::En => "Cancel",
            Self::Ja => "キャンセル",
            Self::Ko => "취소",
            Self::Fr => "Annuler",
            Self::De => "Abbrechen",
            Self::Es => "Cancelar",
            Self::Ru => "Отмена",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_supported_locale_codes_and_roundtrips_u8() {
        let cases = [
            ("zh-CN", UiLocale::ZhCn),
            ("zh-TW", UiLocale::ZhTw),
            ("zh-HK", UiLocale::ZhTw),
            ("en", UiLocale::En),
            ("en-US", UiLocale::En),
            ("ja", UiLocale::Ja),
            ("ko", UiLocale::Ko),
            ("fr", UiLocale::Fr),
            ("de", UiLocale::De),
            ("es", UiLocale::Es),
            ("ru", UiLocale::Ru),
        ];
        for (code, expected) in cases {
            let parsed = UiLocale::from_code(code);
            assert_eq!(parsed, expected);
            assert_eq!(UiLocale::from_code(parsed.code()), expected);
            assert_eq!(UiLocale::from_u8(parsed.as_u8()), expected);
            assert!(!parsed.tray_open_main().is_empty());
            assert!(!parsed.selection_drag().is_empty());
            assert!(!parsed.caption_update().is_empty());
        }
        assert_eq!(UiLocale::resolve(None, Some(true)), UiLocale::En);
        assert_eq!(UiLocale::resolve(None, Some(false)), UiLocale::ZhCn);
        assert_eq!(UiLocale::resolve(Some("ja"), Some(false)), UiLocale::Ja);
    }
}
