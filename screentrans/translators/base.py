from __future__ import annotations

import requests

from ..network import (
    configured_secrets,
    ensure_success,
    json_response,
    redact_sensitive,
    text_response,
)

TIMEOUT = 20


class TranslateError(RuntimeError):
    pass


def friendly(exc: Exception, secrets=()) -> str:
    """把网络异常翻译成人话。

    requests 抛出来的原文长这样：
      SSLError: HTTPSConnectionPool(host='...', port=443): Max retries exceeded with url: ...
    直接怼到译文框里，用户既看不懂也不知道该干什么。
    """
    if isinstance(exc, TranslateError):
        message = str(exc)
    elif isinstance(exc, requests.exceptions.ProxyError):
        message = "连不上翻译服务：代理有问题，检查一下系统代理或科学上网工具"
    elif isinstance(exc, requests.exceptions.SSLError):
        message = "连不上翻译服务：TLS 握手失败，多半是被代理或防火墙挡了"
    elif isinstance(exc, requests.exceptions.ConnectTimeout):
        message = f"连接翻译服务超时（{TIMEOUT} 秒），检查网络或换个接口"
    elif isinstance(exc, requests.exceptions.ReadTimeout):
        message = f"翻译服务响应超时（{TIMEOUT} 秒），可能是这次内容太多，或者线路慢"
    elif isinstance(exc, requests.exceptions.ConnectionError):
        message = "连不上翻译服务：网络不通，或者这个接口在本地被墙了"
    else:
        message = f"{type(exc).__name__}: {exc}"
    return redact_sensitive(message, secrets)


class Translator:
    """所有翻译后端的统一接口。

    实现 translate() 即可：输入多段文本 + 目标语言（通用码 zh-Hans / en / ...），
    返回等长的译文列表。
    """

    name = "base"
    label = "Base"
    needs_key = True

    # 各家接口对「一次请求最多几段 / 多少字符」都有限制。
    # 段落拆得细之后，整屏框选很容易一次几十上百段，必须自动分批。
    max_items = 100
    max_chars = 40000

    # 能不能联网列出可用模型（设置界面据此决定模型栏是输入框还是下拉框）
    supports_model_list = False

    def __init__(self, opts: dict):
        self.opts = opts or {}
        self.cancel_check = lambda: False
        self._http: requests.Session | None = None

    def _raise_if_cancelled(self) -> None:
        if self.cancel_check():
            raise TranslateError("翻译已取消")

    def translate(self, texts: list[str], target: str, source: str | None = None) -> list[str]:
        out: list[str] = []
        for chunk in self._chunks(texts):
            self._raise_if_cancelled()
            translated = self._translate_batch(chunk, target, source)
            self._raise_if_cancelled()
            if len(translated) != len(chunk):
                raise TranslateError(
                    f"翻译接口返回了 {len(translated)} 段，实际需要 {len(chunk)} 段"
                )
            out.extend(translated)
        return out

    def _translate_batch(self, texts: list[str], target: str, source: str | None) -> list[str]:
        raise NotImplementedError

    def _chunks(self, texts: list[str]):
        cur: list[str] = []
        cur_chars = 0
        for t in texts:
            if cur and (len(cur) >= self.max_items or cur_chars + len(t) > self.max_chars):
                yield cur
                cur, cur_chars = [], 0
            cur.append(t)
            cur_chars += len(t)
        if cur:
            yield cur

    # -------------------------------------------------------------- 工具
    def _session(self) -> requests.Session:
        if self._http is None:
            self._http = requests.Session()
            self._http.headers["User-Agent"] = "Mozilla/5.0 ScreenTranslate/1.0"
        return self._http

    def close(self) -> None:
        session = self._http
        self._http = None
        if session is not None:
            session.close()

    def _check(self, resp: requests.Response, label: str = "翻译接口") -> None:
        ensure_success(
            resp,
            label=label,
            error_type=TranslateError,
            secrets=configured_secrets(self.opts),
        )

    @staticmethod
    def _json(resp: requests.Response, label: str = "翻译接口"):
        return json_response(resp, label=label, error_type=TranslateError)

    @staticmethod
    def _text(resp: requests.Response, label: str = "翻译接口") -> str:
        return text_response(resp, label=label, error_type=TranslateError)

    def list_models(self) -> list[str]:
        """列出该接口当前可用的模型名。不支持的后端会抛异常。"""
        raise TranslateError("这个接口不需要选模型")

    def check(self) -> str:
        """设置界面里「测试」按钮用的连通性自检，返回一句人话。"""
        out = self.translate(["Hello, world."], "zh-Hans")
        return f"OK -> {out[0]}"
