fn han(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x2fa1f)
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
            0xac00..=0xd7af | 0x1100..=0x11ff => hangul += 1,
            0x0400..=0x04ff => cyrillic += 1,
            _ if han(c) => chinese += 1,
            _ if c.is_ascii_alphabetic() => latin += 1,
            _ => {}
        }
    }
    if kana > 0 && kana * 12 >= chinese {
        "ja"
    } else if hangul > 0 {
        "ko"
    } else if cyrillic > latin && cyrillic >= 2 {
        "ru"
    } else if chinese >= 2 && chinese * 3 >= latin {
        "zh"
    } else {
        "other"
    }
}

pub fn target_for(text: &str, selected: &str, chinese_target: &str) -> String {
    if selected != "auto" {
        return selected.into();
    }
    if detect(text) == "zh" {
        chinese_target.into()
    } else {
        "zh-Hans".into()
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
        assert_eq!(target_for("Hello world", "auto", "en"), "zh-Hans");
        assert_eq!(target_for("这是中文", "auto", "en"), "en");
        assert_eq!(target_for("これは日本語です", "auto", "en"), "zh-Hans");
        assert_eq!(target_for("안녕하세요", "auto", "en"), "zh-Hans");
        assert_eq!(target_for("这是中文", "fr", "en"), "fr");
    }
}
