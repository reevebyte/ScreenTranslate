from __future__ import annotations

import unittest
import tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch

import requests
from PIL import Image

from qt_helpers import MemoryConfig
from screentrans.network import ensure_success, json_response, redact_sensitive
from screentrans.ocr import azure_vision
from screentrans.ocr.base import OcrError
from screentrans.translators.anthropic import AnthropicTranslator
from screentrans.translators.base import TranslateError, friendly
from screentrans.translators.deepl import DeepLTranslator
from screentrans.translators.google import GoogleTranslator
from screentrans.translators.microsoft import MicrosoftTranslator
from screentrans.translators.openai_like import OpenAILikeTranslator
from screentrans.ui import settings_window
from screentrans.worker import TranslateWorker


def _response(payload, status=200):
    response = Mock(status_code=status, headers={})
    response.json.return_value = payload
    return response


class ConfigurableUrlSecurityTests(unittest.TestCase):
    def test_microsoft_rejects_non_https_credentials_query_and_fragment(self):
        for endpoint in (
            "http://api.example.test",
            "https://user:password@api.example.test",
            "https://api.example.test?target=other",
            "https://api.example.test/#fragment",
        ):
            with self.subTest(endpoint=endpoint):
                engine = MicrosoftTranslator({"key": "secret-key", "endpoint": endpoint})
                with self.assertRaises(TranslateError):
                    engine._translate_batch(["hello"], "zh-Hans", None)
                self.assertIsNone(engine._http)

    def test_anthropic_rejects_non_https_credentials_query_and_fragment(self):
        for base_url in (
            "http://api.example.test",
            "https://user:password@api.example.test",
            "https://api.example.test?route=other",
            "https://api.example.test/#fragment",
        ):
            with self.subTest(base_url=base_url):
                engine = AnthropicTranslator({"key": "secret-key", "base_url": base_url})
                with self.assertRaises(TranslateError):
                    engine._base_and_headers()

    def test_openai_http_is_only_allowed_for_keyless_loopback(self):
        allowed = (
            "http://localhost:11434/v1",
            "http://127.0.0.1:8000/v1",
            "http://127.99.2.3:8000/v1",
            "http://[::1]:8000/v1",
        )
        for base_url in allowed:
            with self.subTest(base_url=base_url):
                self.assertEqual(OpenAILikeTranslator({"base_url": base_url})._root(), base_url)

        rejected = (
            ({"base_url": "http://api.example.test/v1"}, "remote HTTP"),
            ({"base_url": "http://localhost:11434/v1", "key": "secret-key"}, "keyed HTTP"),
            ({"base_url": "http://localhost.example.test/v1"}, "lookalike host"),
            ({"base_url": "https://user@api.example.test/v1"}, "userinfo"),
            ({"base_url": "https://api.example.test/v1?route=x"}, "query"),
            ({"base_url": "https://api.example.test/v1#x"}, "fragment"),
        )
        for options, label in rejected:
            with self.subTest(label=label):
                with self.assertRaises(TranslateError):
                    OpenAILikeTranslator(options)._root()


class KeyedRequestSecurityTests(unittest.TestCase):
    def test_google_key_uses_header_and_never_query_string(self):
        response = _response(
            {"data": {"translations": [{"translatedText": "hello"}]}}
        )
        session = Mock()
        session.post.return_value = response
        engine = GoogleTranslator({"key": "google-secret"})

        with patch.object(engine, "_session", return_value=session):
            self.assertEqual(engine._translate_batch(["你好"], "en", None), ["hello"])

        kwargs = session.post.call_args.kwargs
        self.assertNotIn("params", kwargs)
        self.assertEqual(kwargs["headers"]["X-Goog-Api-Key"], "google-secret")
        self.assertFalse(kwargs["allow_redirects"])
        self.assertTrue(kwargs["stream"])
        self.assertNotIn("google-secret", session.post.call_args.args[0])

    def test_every_user_keyed_provider_disables_redirects_and_streams_json(self):
        cases = []

        microsoft_response = _response([{"translations": [{"text": "hello"}]}])
        microsoft_session = Mock(post=Mock(return_value=microsoft_response))
        microsoft = MicrosoftTranslator(
            {"key": "secret-key", "endpoint": "https://api.example.test"}
        )
        cases.append((microsoft, microsoft_session, "post", lambda: microsoft._translate_batch(["x"], "en", None)))

        deepl_response = _response({"translations": [{"text": "hello"}]})
        deepl_session = Mock(post=Mock(return_value=deepl_response))
        deepl = DeepLTranslator({"key": "secret-key"})
        cases.append((deepl, deepl_session, "post", lambda: deepl._translate_batch(["x"], "en", None)))

        openai_response = _response({"data": [{"id": "model-a"}]})
        openai_session = Mock(get=Mock(return_value=openai_response))
        openai = OpenAILikeTranslator(
            {"base_url": "https://api.example.test/v1", "key": "secret-key"}
        )
        cases.append((openai, openai_session, "get", openai.list_models))

        anthropic_response = _response({"data": [{"id": "model-a"}]})
        anthropic_session = Mock(get=Mock(return_value=anthropic_response))
        anthropic = AnthropicTranslator(
            {"base_url": "https://api.example.test", "key": "secret-key"}
        )
        cases.append((anthropic, anthropic_session, "get", anthropic.list_models))

        for engine, session, method, call in cases:
            with self.subTest(provider=engine.name):
                with patch.object(engine, "_session", return_value=session):
                    call()
                kwargs = getattr(session, method).call_args.kwargs
                self.assertFalse(kwargs["allow_redirects"])
                self.assertTrue(kwargs["stream"])

    def test_malformed_success_payload_never_echoes_provider_key(self):
        secret = "ordinary-google-style-secret-value"
        reflected = ("x" * 150) + secret

        openai = OpenAILikeTranslator(
            {
                "base_url": "https://api.example.test/v1",
                "key": secret,
                "model": "model-a",
            }
        )
        openai_session = Mock(
            post=Mock(return_value=_response({"unexpected": reflected})),
            get=Mock(return_value=_response({"unexpected": reflected})),
        )
        anthropic = AnthropicTranslator(
            {
                "base_url": "https://api.example.test",
                "key": secret,
                "model": "model-a",
            }
        )
        anthropic_session = Mock(
            post=Mock(return_value=_response({"unexpected": reflected}))
        )

        calls = (
            (openai, openai_session, lambda: openai._call("system", "user")),
            (openai, openai_session, openai.list_models),
            (anthropic, anthropic_session, lambda: anthropic._call("system", "user")),
        )
        for engine, session, call in calls:
            with self.subTest(provider=engine.name, operation=call):
                with patch.object(engine, "_session", return_value=session):
                    with self.assertRaises(TranslateError) as caught:
                        call()
                message = str(caught.exception)
                self.assertNotIn(secret, message)
                self.assertNotIn(secret[:12], message)
                self.assertNotIn(reflected[:20], message)


class AzureOperationSecurityTests(unittest.TestCase):
    def test_operation_location_must_be_https_and_same_origin(self):
        for operation_url in (
            "http://vision.example.test/result/123",
            "https://attacker.example/result/123",
            "https://user@vision.example.test/result/123",
        ):
            with self.subTest(operation_url=operation_url):
                submitted = _response({}, status=202)
                submitted.headers = {"Operation-Location": operation_url}
                session = Mock(post=Mock(return_value=submitted))
                with self.assertRaises(OcrError):
                    azure_vision.recognize(
                        Image.new("RGB", (20, 20), "white"),
                        {"endpoint": "https://vision.example.test", "key": "secret-key"},
                        session=session,
                    )
                session.get.assert_not_called()
                kwargs = session.post.call_args.kwargs
                self.assertFalse(kwargs["allow_redirects"])
                self.assertTrue(kwargs["stream"])

    def test_failed_operation_does_not_echo_key(self):
        secret = "azure-secret-value"
        submitted = _response({}, status=202)
        submitted.headers = {
            "Operation-Location": "https://vision.example.test/result/123"
        }
        failed = _response(
            {"status": "failed", "error": {"message": secret}},
            status=200,
        )
        session = Mock(
            post=Mock(return_value=submitted),
            get=Mock(return_value=failed),
        )

        with self.assertRaises(OcrError) as caught:
            azure_vision.recognize(
                Image.new("RGB", (20, 20), "white"),
                {"endpoint": "https://vision.example.test", "key": secret},
                session=session,
            )

        self.assertNotIn(secret, str(caught.exception))
        self.assertIn("[REDACTED]", str(caught.exception))


class BoundedResponseAndRedactionTests(unittest.TestCase):
    def test_json_reader_rejects_body_over_limit(self):
        response = requests.Response()
        response.status_code = 200
        response._content = b'{"text":"' + (b"x" * 64) + b'"}'

        with self.assertRaisesRegex(TranslateError, "响应过大"):
            json_response(
                response,
                label="test",
                error_type=TranslateError,
                limit=32,
            )

    def test_error_text_redacts_known_keys_headers_and_query_parameters(self):
        secret = "super-secret-value"
        raw = (
            "GET https://example.test/v1?key=old-query-secret "
            f"Authorization: Bearer {secret} sk-ant-api-secret"
        )
        safe = redact_sensitive(raw, [secret])
        self.assertNotIn(secret, safe)
        self.assertNotIn("old-query-secret", safe)
        self.assertNotIn("sk-ant-api-secret", safe)
        self.assertIn("[REDACTED]", safe)
        self.assertNotIn(secret, friendly(TranslateError(raw), [secret]))

    def test_redaction_happens_before_error_detail_is_truncated(self):
        secret = "boundary-secret-value"
        response = Mock(status_code=400, text=("x" * 295) + secret, headers={})
        with self.assertRaises(TranslateError) as caught:
            ensure_success(
                response,
                label="test",
                error_type=TranslateError,
                secrets=[secret],
            )
        self.assertNotIn(secret, str(caught.exception))
        self.assertNotIn(secret[:5], str(caught.exception))

    def test_escaped_control_characters_in_secret_are_redacted(self):
        secret = "header-key\nsecond-line"
        escaped = repr(secret)[1:-1]
        safe = redact_sensitive(f"invalid header value: '{escaped}'", [secret])
        self.assertNotIn(escaped, safe)
        self.assertIn("[REDACTED]", safe)

    def test_runtime_worker_redacts_provider_key_from_failure(self):
        secret = "runtime-key\nsecond-line"
        cfg = MemoryConfig(
            {
                "lang": {"zh_target": "en"},
                "translator": {"provider": "fake", "fake": {"key": secret}},
            }
        )
        worker = TranslateWorker(None, cfg, [SimpleNamespace(text="hello")])
        failures = []
        worker.failed.connect(failures.append)
        engine = Mock()
        engine.translate.side_effect = requests.exceptions.InvalidHeader(
            f"invalid header value: {secret!r}"
        )

        with patch("screentrans.worker.translators.build", return_value=engine):
            worker.run()

        self.assertEqual(len(failures), 1)
        self.assertNotIn("runtime-key", failures[0])
        self.assertNotIn("second-line", failures[0])
        self.assertIn("[REDACTED]", failures[0])

    def test_online_selftest_redacts_key_and_closes_engine(self):
        from screentrans import config as config_module

        secret = "selftest-key\nsecond-line"
        cfg = MemoryConfig(
            {
                "ocr": {
                    "engine": "windows",
                    "azure_vision": {"key": "azure-unused-secret"},
                },
                "appearance": {"font_family": "Microsoft YaHei UI"},
                "translator": {"provider": "fake", "fake": {"key": secret}},
            }
        )
        engine = Mock()
        engine.check.side_effect = requests.exceptions.InvalidHeader(
            f"invalid header value: {secret!r}"
        )
        hotkeys = Mock()
        hotkeys.register.return_value = (True, "")

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(config_module, "CONFIG_PATH", root / "missing-config.json"):
                from screentrans import main as main_module
            with (
                patch.object(main_module, "QApplication", return_value=Mock()),
                patch.object(main_module, "Config", return_value=cfg),
                patch.object(main_module, "HotkeyHost", return_value=hotkeys),
                patch.object(main_module.sys, "argv", ["ScreenTranslate.exe", "--quiet"]),
                patch.object(config_module, "CONFIG_DIR", root),
                patch.object(config_module, "CONFIG_PATH", root / "config.json"),
                patch("screentrans.ocr.windows_ocr.available_languages", return_value=[]),
                patch("screentrans.ocr.recognize", return_value=[]),
                patch("screentrans.render.render"),
                patch("screentrans.translators.build", return_value=engine),
                patch("builtins.print"),
            ):
                main_module.selftest()
            report = (root / "selftest.txt").read_text(encoding="utf-8")

        self.assertNotIn("selftest-key", report)
        self.assertNotIn("second-line", report)
        self.assertIn("[REDACTED]", report)
        engine.close.assert_called_once_with()

    def test_settings_test_and_model_workers_emit_only_redacted_errors(self):
        secret = "settings-secret-value"
        engine = Mock()
        engine.check.side_effect = TranslateError(
            f"request failed: https://example.test/?key={secret}"
        )
        engine.list_models.side_effect = RuntimeError(
            f"Authorization: Bearer {secret}"
        )

        for worker_type in (settings_window._TestThread, settings_window._ModelsThread):
            with self.subTest(worker=worker_type.__name__):
                messages = []
                worker = worker_type("openai", {"key": secret})
                worker.done.connect(lambda ok, message: messages.append((ok, message)))
                with patch.object(settings_window.translators, "build", return_value=engine):
                    worker.run()
                self.assertEqual(len(messages), 1)
                self.assertFalse(messages[0][0])
                self.assertNotIn(secret, str(messages[0][1]))
                self.assertIn("[REDACTED]", str(messages[0][1]))


if __name__ == "__main__":
    unittest.main()
