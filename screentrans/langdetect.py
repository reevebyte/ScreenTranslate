"""判断原文语种，决定翻译目标语言。

规则（对应需求：框选外文 → 中文；框选中文 → 英文）：
  含假名   -> 日文 -> 译成中文
  含谚文   -> 韩文 -> 译成中文
  汉字为主 -> 中文 -> 译成英文（目标语言可在设置里改）
  其余     ->      -> 译成中文
"""
from __future__ import annotations

import re


def script_counts(text: str) -> dict[str, int]:
    c = {"han": 0, "kana": 0, "hangul": 0, "latin": 0, "cyrillic": 0, "other": 0}
    for ch in text:
        o = ord(ch)
        if 0x3040 <= o <= 0x30FF or 0x31F0 <= o <= 0x31FF:
            c["kana"] += 1
        elif 0xAC00 <= o <= 0xD7AF or 0x1100 <= o <= 0x11FF:
            c["hangul"] += 1
        elif 0x3400 <= o <= 0x4DBF or 0x4E00 <= o <= 0x9FFF or 0xF900 <= o <= 0xFAFF:
            c["han"] += 1
        elif 0x0400 <= o <= 0x04FF:
            c["cyrillic"] += 1
        elif ch.isalpha() and o < 0x0250:
            c["latin"] += 1
        elif not ch.isspace():
            c["other"] += 1
    return c


def _latin_evidence(text: str, raw_count: int) -> int:
    """估算真正代表英文句子的字母数，弱化 URL、路径和代码标识符。"""
    evidence = raw_count

    def is_han(ch: str) -> bool:
        o = ord(ch)
        return 0x3400 <= o <= 0x4DBF or 0x4E00 <= o <= 0x9FFF or 0xF900 <= o <= 0xFAFF

    for match in re.finditer(r"[A-Za-z][A-Za-z0-9_./:\\-]*", text):
        token = match.group(0)
        letters = [ch for ch in token if ch.isalpha() and ch.isascii()]
        if not letters:
            continue
        before = text[match.start() - 1] if match.start() else ""
        after = text[match.end()] if match.end() < len(text) else ""
        camel = sum(ch.isupper() for ch in letters) >= 2 and any(ch.islower() for ch in letters)
        code_marks = any(ch.isdigit() or ch in "_./:\\-" for ch in token)
        touches_han = bool(before and is_han(before) or after and is_han(after))
        if camel or code_marks or touches_han:
            evidence -= max(0, len(letters) - 2)
    return max(0, evidence)


def detect(text: str) -> str:
    """返回粗略语种：zh / ja / ko / ru / other"""
    c = script_counts(text)
    if c["kana"] >= 1 and c["kana"] * 12 >= c["han"]:
        return "ja"
    if c["hangul"] >= 1:
        return "ko"
    if c["cyrillic"] > c["latin"] and c["cyrillic"] >= 2:
        return "ru"
    # 汉字的信息密度比拉丁字母高；3 倍权重能识别「用户设置abcdefghijk」这类
    # 中文夹标识符，同时不会把一整段英文末尾偶尔出现的「中文」两字判成中文页。
    latin = _latin_evidence(text, c["latin"])
    if c["han"] >= 2 and c["han"] * 3 >= latin:
        return "zh"
    return "other"


def target_for(text: str, zh_target: str = "en") -> tuple[str, str]:
    """返回 (源语种, 目标语言代码)。目标语言用通用码：zh-Hans / en / ..."""
    src = detect(text)
    if src == "zh":
        allowed = {"en", "ja", "ko", "fr", "de", "es", "ru", "zh-Hant"}
        return src, zh_target if zh_target in allowed else "en"
    return src, "zh-Hans"


def target_display_name(code: str) -> str:
    """给人看的名字（界面、通知里用）。"""
    return {
        "zh-Hans": "简体中文",
        "zh-Hant": "繁体中文",
        "en": "英语",
        "ja": "日语",
        "ko": "韩语",
        "fr": "法语",
        "de": "德语",
        "es": "西班牙语",
        "ru": "俄语",
    }.get(code, code)


_ENGLISH_NAME = {
    "zh-Hans": "Simplified Chinese",
    "zh-Hant": "Traditional Chinese",
    "en": "English",
    "ja": "Japanese",
    "ko": "Korean",
    "fr": "French",
    "de": "German",
    "es": "Spanish",
    "ru": "Russian",
}


def target_prompt_name(code: str) -> str:
    """给**模型**看的名字。

    必须是英文。以前这里直接用了界面上那个中文名，提示词写出来是
    「Translate every segment into 英语」——一整段英文指令里夹一个中文词，
    再配上一整屏中文原文，小模型（8B 那一档）很容易就顺着中文回中文了。
    这是小模型偶尔顺着中文原文继续回中文的一个明显诱因。
    """
    return _ENGLISH_NAME.get(code, code)


# 每种目标语言「写出来应该长什么样」。只看字符集，不做真正的语种识别——
# 要抓的是「让它译成英语、它回了一整段中文」这种一眼可见的失败，够用了。
_WANT_SCRIPT = {
    "en": "latin", "fr": "latin", "de": "latin", "es": "latin",
    "it": "latin", "pt": "latin", "nl": "latin", "vi": "latin",
    "zh-Hans": "han", "zh-Hant": "han",
    "ja": "kana", "ko": "hangul", "ru": "cyrillic",
}


def matches_target(text: str, target: str) -> bool:
    """这段译文看起来确实是目标语言吗。看不出问题就返回 True（宁可放过不可错杀）。"""
    want = _WANT_SCRIPT.get(target)
    if not want:
        return True
    c = script_counts(text)
    cjk = c["han"] + c["kana"] + c["hangul"]
    if want == "latin":
        # 拉丁语系的译文里混几个汉字（专有名词）没关系，成段的中日韩才是没照做
        return not (cjk >= 1 and cjk * 2 >= c["latin"])
    letters = cjk + c["latin"] + c["cyrillic"]
    if letters == 0:
        return True          # 纯数字、标点，语言判不了也不用判
    if want == "han":
        return c["han"] >= 1 or (letters == 1 and c["latin"] == 1)
    if want == "kana":
        return c["kana"] >= 1 or c["han"] >= 1 or (letters == 1 and c["latin"] == 1)
    if want == "hangul":
        return c["hangul"] >= 1 or (letters == 1 and c["latin"] == 1)
    if want == "cyrillic":
        return c["cyrillic"] >= 1 or (letters == 1 and c["latin"] == 1)
    return True
