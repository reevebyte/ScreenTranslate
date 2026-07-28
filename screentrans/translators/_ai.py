"""大模型翻译的公共部分：提示词 + 批量协议 + 容错解析。

批量协议：把 N 段原文以 JSON 数组送进去，要求模型返回等长的 JSON 数组。
模型偶尔会多话或漏条目，所以解析失败时会自动退化成逐段翻译。
"""
from __future__ import annotations

import json
import re

from ..langdetect import matches_target, target_prompt_name
from .base import TranslateError

# 目标语言的名字必须写英文。以前这里塞的是界面上那个中文名（「英语」），
# 一整段英文指令里夹一个中文词，配上满屏中文原文，8B 那一档的模型
# 十次里有几次就顺着中文回中文了。
SYSTEM_PROMPT = (
    "You are a professional translation engine embedded in a screen-translation tool. "
    "You will receive a JSON array of text segments extracted from a screenshot.\n"
    "Rules:\n"
    "1. Translate every segment into {target}. The output text MUST be written in "
    "{target}. Never answer in the source language, and never explain or comment.\n"
    "2. Return ONLY a JSON array of strings, same length and same order as the input. "
    "No markdown fences, no commentary, no keys.\n"
    "3. Keep the translation concise and natural — it will be drawn back on top of the "
    "original text in a limited area, so avoid padding.\n"
    "4. Preserve numbers, code identifiers, URLs, file paths and proper nouns as-is when "
    "translation would harm clarity.\n"
    "5. If a segment is already written in {target}, return it unchanged.\n"
    "6. Never merge or split segments. An empty input segment maps to an empty string."
)

# 第一遍没照做时用这句再来一次。说得又短又硬，比原来那六条更难跑偏。
RETRY_PROMPT = (
    "Translate the following JSON array into {target}.\n"
    "The previous attempt failed because the output was NOT in {target}.\n"
    "Every returned string must be written in {target} — this is the only thing that "
    "matters. Return ONLY a JSON array of strings of the same length. No other text."
)


def build_messages(texts: list[str], target: str,
                   template: str = SYSTEM_PROMPT) -> tuple[str, str]:
    system = template.format(target=target_prompt_name(target))
    user = json.dumps(texts, ensure_ascii=False)
    return system, user


def parse_array(raw: str, expected: int) -> list[str] | None:
    """从模型输出里抠出 JSON 数组。抠不出来或条数不对就返回 None。"""
    if not raw:
        return None
    text = raw.strip()
    text = re.sub(r"^```(?:json)?\s*|\s*```$", "", text, flags=re.I | re.M).strip()
    start, end = text.find("["), text.rfind("]")
    if start == -1 or end <= start:
        return None
    try:
        data = json.loads(text[start : end + 1])
    except Exception:
        return None
    if (not isinstance(data, list) or len(data) != expected
            or any(not isinstance(value, str) for value in data)):
        return None
    return data


def batched(texts: list[str], call, target: str) -> list[str]:
    """call(system, user) -> 模型输出文本。先批量，失败再逐段，最后校一遍语言。"""
    if not texts:
        return []
    system, user = build_messages(texts, target)
    try:
        parsed = parse_array(call(system, user), len(texts))
    except TranslateError:
        raise
    except Exception as exc:
        raise TranslateError(str(exc)) from exc

    if parsed is None:
        # 退化路径：一段一段来，慢但稳
        parsed = []
        for t in texts:
            if not t.strip():
                parsed.append("")
                continue
            s, u = build_messages([t], target)
            raw = call(s, u)
            one = parse_array(raw, 1)
            parsed.append(one[0] if one else _plain(raw))

    return _enforce_language(texts, parsed, call, target)


def _enforce_language(texts: list[str], out: list[str], call, target: str) -> list[str]:
    """挑出「没译成目标语言」的那些段，换一句硬话再来一次。

    光靠提示词管不住小模型——它认了六条规则，下一次照样给你回一段中文。
    真正靠得住的是**看结果**：译文里成段的中日韩字符还在，就是没照做。
    只重来一次；还不行就明确报错，不能把中文原样盖回去冒充英文译文。
    """
    bad = [
        i for i, (source, translated) in enumerate(zip(texts, out))
        if source.strip() and not _valid_output(source, translated, target)
    ]
    if not bad:
        return out

    print(f"[translate] {len(bad)}/{len(out)} 段没译成{target_prompt_name(target)}，"
          "换硬提示词重来一次")
    try:
        s, u = build_messages([texts[i] for i in bad], target, RETRY_PROMPT)
        again = parse_array(call(s, u), len(bad))
    except Exception as exc:
        raise TranslateError(
            f"模型没有按要求输出{target_prompt_name(target)}，重试也失败了：{exc}"
        ) from exc
    if again is None:
        raise TranslateError(
            f"模型没有按要求输出{target_prompt_name(target)}，重试返回格式也不正确"
        )
    for slot, fixed in zip(bad, again):
        # 只在真的改好了的时候才换掉，别拿一个同样不对的覆盖原来的
        if _valid_output(texts[slot], fixed, target):
            out[slot] = fixed
    remaining = [
        slot for slot in bad
        if not _valid_output(texts[slot], out[slot], target)
    ]
    if remaining:
        raise TranslateError(
            f"模型连续两次没有输出{target_prompt_name(target)}，请重试或换一个模型"
        )
    return out


def _valid_output(source: str, translated: str, target: str) -> bool:
    """协议、内容和目标文字系统都至少说得通。"""
    if not translated.strip():
        return False
    # 有文字的原文不应被模型“翻译”成纯数字/标点；短输出仅靠文字系统判不了。
    if any(ch.isalpha() for ch in source) and not any(ch.isalpha() for ch in translated):
        return False
    if translated.strip() == source.strip() and _preservable(source):
        return True
    return matches_target(translated, target)


def _preservable(text: str) -> bool:
    """无需翻译的短代码、缩写、URL、路径等。"""
    value = text.strip()
    if not value or not value.isascii() or any(ch.isspace() for ch in value):
        return False
    letters = [ch for ch in value if ch.isalpha()]
    if not letters:
        return True
    return (
        len(letters) == 1
        or all(ch.isupper() for ch in letters)
        or sum(ch.isupper() for ch in letters) >= 2
        or any(ch.isdigit() or ch in "_./:\\-" for ch in value)
    )


def _plain(raw: str) -> str:
    text = re.sub(
        r"^```(?:json)?\s*|\s*```$", "", (raw or "").strip(), flags=re.I | re.M
    ).strip()
    try:
        value = json.loads(text)
    except Exception:
        return text
    # 退化路径只接受一个普通字符串。数组、对象、数字等协议错误交给语言校验重试。
    return value if isinstance(value, str) else ""
