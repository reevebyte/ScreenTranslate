from __future__ import annotations

import unittest
from unittest.mock import Mock, patch

from screentrans import translators
from screentrans.translators.free_web import (
    BingFreeTranslator,
    IcibaFreeTranslator,
    MicrosoftFreeTranslator,
    TencentFreeTranslator,
    YandexFreeTranslator,
)


def _response(payload=None, *, text="", url="https://example.test", status=200):
    response = Mock(status_code=status, text=text, url=url)
    response.json.return_value = payload
    return response


class FreeTranslatorRegistryTests(unittest.TestCase):
    def test_all_free_engines_are_registered_without_configuration_fields(self):
        for name in (
            "google_free",
            "bing_free",
            "microsoft_free",
            "tencent_free",
            "yandex_free",
            "iciba_free",
        ):
            with self.subTest(name=name):
                self.assertIn(name, translators.PROVIDERS)
                self.assertFalse(translators.PROVIDERS[name].needs_key)
                self.assertEqual(translators.FIELDS[name], [])


class FreeTranslatorResponseTests(unittest.TestCase):
    def test_bing_fetches_web_token_once_and_preserves_block_order(self):
        html = (
            'params_AbusePreventionHelper = [12345,"token-value",0];'
            'IG:"ig-value" data-iid="translator.777"'
        )
        session = Mock()
        session.get.return_value = _response(
            text=html,
            # 即便测试替身声称最终地址变了，后续原文也只能发往固定的 Bing 主机。
            url="http://attacker.example/translator",
        )
        session.post.side_effect = [
            _response([{"translations": [{"text": "甲"}]}]),
            _response([{"translations": [{"text": "乙"}]}]),
        ]
        engine = BingFreeTranslator({})
        with patch.object(engine, "_session", return_value=session):
            self.assertEqual(engine.translate(["A", "B"], "zh-Hans"), ["甲", "乙"])
        self.assertEqual(session.get.call_count, 1)
        self.assertFalse(session.get.call_args.kwargs["allow_redirects"])
        self.assertTrue(
            all(
                call.args[0] == "https://cn.bing.com/ttranslatev3"
                for call in session.post.call_args_list
            )
        )

    def test_microsoft_free_adds_client_signature(self):
        session = Mock()
        session.post.return_value = _response(
            [{"translations": [{"text": "你好"}]}]
        )
        engine = MicrosoftFreeTranslator({})
        with patch.object(engine, "_session", return_value=session):
            self.assertEqual(engine.translate(["Hello"], "zh-Hans"), ["你好"])
        headers = session.post.call_args.kwargs["headers"]
        self.assertTrue(headers["X-MT-Signature"].startswith("MSTranslatorAndroidApp::"))

    def test_tencent_accepts_translation_blocks(self):
        session = Mock()
        session.post.return_value = _response({"auto_translation": ["你", "好"]})
        engine = TencentFreeTranslator({})
        with patch.object(engine, "_session", return_value=session):
            self.assertEqual(engine.translate(["Hello"], "zh-Hans"), ["你\n好"])

    def test_yandex_reads_first_translation(self):
        session = Mock()
        session.post.return_value = _response({"text": ["你好"]})
        engine = YandexFreeTranslator({})
        with patch.object(engine, "_session", return_value=session):
            self.assertEqual(engine.translate(["Hello"], "zh-Hans"), ["你好"])

    def test_iciba_batches_blocks_and_preserves_count(self):
        session = Mock()
        session.post.return_value = _response(
            {"code": 1, "data": [{"out": "甲"}, {"out": "乙"}]}
        )
        engine = IcibaFreeTranslator({})
        with patch.object(engine, "_session", return_value=session):
            self.assertEqual(engine.translate(["A", "B"], "zh-Hans"), ["甲", "乙"])
        self.assertEqual(session.post.call_args.kwargs["json"]["textList"], ["A", "B"])


if __name__ == "__main__":
    unittest.main()
