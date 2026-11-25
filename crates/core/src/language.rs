fn han(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x2fa1f)
}

fn is_latin(c: char) -> bool {
    c.is_ascii_alphabetic()
        || matches!(
            c as u32,
            0x00C0..=0x00D6 | 0x00D8..=0x00F6 | 0x00F8..=0x024F
        )
}

const SIMPLIFIED_MARKERS: &str = "这为国们说时会经发进动现实机电语译小写读书网点开关题体简汉选图设键显应认对无与从来没过还让给样东车长见听问间门员处务业产变义边头买卖记话谢请欢乐爱梦飞鸟鱼龙岁钟钱铁银错难离虽须顺领考验惊齐";
const TRADITIONAL_MARKERS: &str = "這為國們說時會經發進動現實機電語譯寫讀書網點開關題體簡漢選圖設鍵顯應認對無與從來沒過還讓給樣東車長見聽問間門員處務業產變義邊頭買賣記話謝請歡樂愛夢飛鳥魚龍歲鐘錢鐵銀錯難離雖須順領考驗驚齊";

fn detect_chinese_variant(text: &str) -> &'static str {
    let mut simplified = 0usize;
    let mut traditional = 0usize;
    for c in text.chars() {
        if SIMPLIFIED_MARKERS.contains(c) {
            simplified += 1;
        } else if TRADITIONAL_MARKERS.contains(c) {
            traditional += 1;
        }
    }
    if traditional > simplified {
        "zh-Hant"
    } else if simplified > traditional {
        "zh-Hans"
    } else {
        "zh"
    }
}

const EN_WORDS: &[&str] = &[
    "the", "and", "for", "that", "with", "this", "from", "are", "was", "were", "have", "has",
    "not", "you", "your", "will", "can", "what", "when", "where", "which", "about", "into", "more",
    "there", "their", "would", "could", "should", "been", "they", "them", "hello", "world",
    "please", "thank", "thanks", "open", "close", "file", "edit", "view", "help", "settings",
    "save", "cancel", "error", "copy", "paste", "search", "update",
];

const FR_WORDS: &[&str] = &[
    "le",
    "la",
    "les",
    "des",
    "une",
    "est",
    "sont",
    "dans",
    "pour",
    "pas",
    "que",
    "qui",
    "avec",
    "sur",
    "vous",
    "nous",
    "mais",
    "plus",
    "comme",
    "tout",
    "faire",
    "cette",
    "elle",
    "ils",
    "elles",
    "aux",
    "par",
    "très",
    "bonjour",
    "merci",
    "fichier",
    "ouvrir",
    "fermer",
    "paramètres",
    "enregistrer",
    "annuler",
];

const DE_WORDS: &[&str] = &[
    "der",
    "die",
    "das",
    "und",
    "ist",
    "sind",
    "nicht",
    "ein",
    "eine",
    "einem",
    "einen",
    "einer",
    "mit",
    "auf",
    "für",
    "von",
    "den",
    "dem",
    "sich",
    "auch",
    "als",
    "noch",
    "nach",
    "wie",
    "aber",
    "oder",
    "wenn",
    "über",
    "wird",
    "werden",
    "haben",
    "kann",
    "hallo",
    "danke",
    "bitte",
    "datei",
    "öffnen",
    "schließen",
    "einstellungen",
    "speichern",
    "abbrechen",
];

const ES_WORDS: &[&str] = &[
    "el",
    "los",
    "las",
    "del",
    "uno",
    "por",
    "para",
    "con",
    "como",
    "pero",
    "más",
    "este",
    "esta",
    "estos",
    "estas",
    "son",
    "está",
    "están",
    "muy",
    "también",
    "todo",
    "todos",
    "tiene",
    "hacer",
    "puede",
    "desde",
    "hasta",
    "hola",
    "gracias",
    "archivo",
    "abrir",
    "cerrar",
    "configuración",
    "guardar",
    "cancelar",
];

fn detect_latin(text: &str) -> &'static str {
    let mut en = 0u32;
    let mut fr = 0u32;
    let mut de = 0u32;
    let mut es = 0u32;

    for c in text.chars() {
        match c {
            'ß' => de += 8,
            'ä' | 'ö' | 'ü' | 'Ä' | 'Ö' | 'Ü' => de += 4,
            'ñ' | 'Ñ' | '¿' | '¡' => es += 8,
            'ç' | 'Ç' | 'œ' | 'Œ' | 'æ' | 'Æ' | '«' | '»' => fr += 6,
            'à' | 'â' | 'è' | 'ê' | 'ë' | 'î' | 'ï' | 'ô' | 'ù' | 'û' | 'À' | 'Â' | 'È' | 'Ê'
            | 'Ë' | 'Î' | 'Ï' | 'Ô' | 'Ù' | 'Û' => fr += 3,
            'é' | 'É' => {
                fr += 2;
                es += 1;
            }
            'á' | 'í' | 'ó' | 'ú' | 'Á' | 'Í' | 'Ó' | 'Ú' => es += 2,
            _ => {}
        }
    }

    for token in text.split(|c: char| !c.is_alphabetic()) {
        if token.is_empty() {
            continue;
        }
        let lower = token.to_lowercase();
        let word = lower.as_str();
        if EN_WORDS.contains(&word) {
            en += 6;
        }
        if FR_WORDS.contains(&word) {
            fr += 6;
        }
        if DE_WORDS.contains(&word) {
            de += 6;
        }
        if ES_WORDS.contains(&word) {
            es += 6;
        }
    }

    let scores = [("en", en), ("fr", fr), ("de", de), ("es", es)];
    let mut best = ("en", 0u32);
    for (lang, score) in scores {
        if score > best.1 {
            best = (lang, score);
        }
    }
    best.0
}

pub fn detect(text: &str) -> &'static str {
    let mut chinese = 0;
    let mut latin = 0;
    let mut kana = 0;
    let mut hangul = 0;
    let mut cyrillic = 0;
    for c in text.chars() {
        match c as u32 {
            0x3040..=0x30ff | 0x31f0..=0x31ff => kana += 1,
            0xac00..=0xd7af | 0x1100..=0x11ff | 0x3130..=0x318f => hangul += 1,
            0x0400..=0x04ff => cyrillic += 1,
            _ if han(c) => chinese += 1,
            _ if is_latin(c) => latin += 1,
            _ => {}
        }
    }
    if kana > 0 && kana * 12 >= chinese {
        "ja"
    } else if hangul > 0 {
        "ko"
    } else if cyrillic > latin && cyrillic >= 2 {
        "ru"
    } else if (chinese >= 2 && chinese * 3 >= latin) || (chinese == 1 && latin == 0) {
        detect_chinese_variant(text)
    } else if latin > 0 {
        detect_latin(text)
    } else {
        "other"
    }
}

fn is_chinese(code: &str) -> bool {
    matches!(code, "zh" | "zh-Hans" | "zh-Hant")
}

fn matches_primary(detected: &str, primary_target: &str, secondary_target: &str) -> bool {
    if detected == primary_target {
        return true;
    }
    if is_chinese(detected) && is_chinese(primary_target) {
        if is_chinese(secondary_target) {
            return detected == "zh";
        }
        return true;
    }
    false
}

pub fn target_for(
    text: &str,
    selected: &str,
    primary_target: &str,
    secondary_target: &str,
) -> String {
    if selected != "auto" {
        return selected.into();
    }
    if matches_primary(detect(text), primary_target, secondary_target) {
        secondary_target.into()
    } else {
        primary_target.into()
    }
}

pub fn prompt_name(code: &str) -> &str {
    match code {
        "zh-Hans" | "zh" => "Simplified Chinese",
        "zh-Hant" => "Traditional Chinese",
        "en" => "English",
        "ja" => "Japanese",
        "ko" => "Korean",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "ru" => "Russian",
        _ => code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_cjk_and_respects_explicit_target() {
        assert_eq!(
            target_for("Hello world", "auto", "zh-Hans", "en"),
            "zh-Hans"
        );
        assert_eq!(target_for("这是中文", "auto", "zh-Hans", "en"), "en");
        assert_eq!(target_for("這是繁體中文", "auto", "zh-Hans", "en"), "en");
        assert_eq!(
            target_for("これは日本語です", "auto", "zh-Hans", "en"),
            "zh-Hans"
        );
        assert_eq!(target_for("안녕하세요", "auto", "zh-Hans", "en"), "zh-Hans");
        assert_eq!(target_for("这是中文", "fr", "zh-Hans", "en"), "fr");
    }

    #[test]
    fn supports_non_chinese_primary_and_latin_language_pairs() {
        assert_eq!(
            target_for("Hello world", "auto", "en", "zh-Hans"),
            "zh-Hans"
        );
        assert_eq!(target_for("这是中文", "auto", "en", "zh-Hans"), "en");
        assert_eq!(target_for("これは日本語です", "auto", "ja", "en"), "en");
        assert_eq!(target_for("Hello world", "auto", "ja", "en"), "ja");
        assert_eq!(target_for("Привет мир", "auto", "ru", "en"), "en");
        assert_eq!(target_for("Hello world", "auto", "ru", "en"), "ru");
        assert_eq!(
            target_for("Bonjour, comment allez-vous ?", "auto", "fr", "en"),
            "en"
        );
        assert_eq!(target_for("Hello, how are you?", "auto", "fr", "en"), "fr");
        assert_eq!(
            target_for("Das ist ein großes Haus", "auto", "de", "en"),
            "en"
        );
        assert_eq!(target_for("This is a big house", "auto", "de", "en"), "de");
        assert_eq!(target_for("¿Cómo estás hoy?", "auto", "es", "en"), "en");
        assert_eq!(target_for("How are you today?", "auto", "es", "en"), "es");
    }

    #[test]
    fn routes_simplified_and_traditional_chinese_pair() {
        assert_eq!(
            target_for("这是简体中文设置", "auto", "zh-Hans", "zh-Hant"),
            "zh-Hant"
        );
        assert_eq!(
            target_for("這是繁體中文設置", "auto", "zh-Hans", "zh-Hant"),
            "zh-Hans"
        );
        assert_eq!(target_for("中文", "auto", "zh-Hans", "zh-Hant"), "zh-Hant");
    }
}
