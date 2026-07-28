from __future__ import annotations

import unittest

from screentrans.langdetect import matches_target, target_for
from screentrans.translators._ai import batched, parse_array
from screentrans.translators.base import TranslateError


class LanguageDetectionRegressionTests(unittest.TestCase):
    def test_chinese_with_long_latin_identifier_targets_english(self):
        text = "\u6d4b\u8bd5 PyInstallerBootloaderConfigurationIdentifier"
        source, target = target_for(text, "en")
        self.assertEqual(source, "zh")
        self.assertEqual(target, "en")

    def test_long_english_sentence_with_two_han_characters_targets_chinese(self):
        text = (
            "This is a long English sentence describing a configuration option "
            "that should remain classified as English despite the label \u6d4b\u8bd5."
        )
        _source, target = target_for(text, "en")
        self.assertEqual(target, "zh-Hans")

    def test_han_output_does_not_match_english_target(self):
        self.assertFalse(matches_target("\u6d4b\u8bd5", "en"))

    def test_short_latin_output_does_not_match_chinese_target(self):
        self.assertFalse(matches_target("Hi", "zh-Hans"))


class AiTranslationRegressionTests(unittest.TestCase):
    def test_parse_array_rejects_non_string_items(self):
        self.assertEqual(parse_array('["ok"]', 1), ["ok"])
        for raw in ('[{"text": "ok"}]', "[1]", "[null]"):
            with self.subTest(raw=raw):
                self.assertIsNone(parse_array(raw, 1))

    def test_wrong_language_first_attempt_is_retried_successfully(self):
        replies = iter(('["\\u6d4b\\u8bd5"]', '["This is a test."]'))
        calls = []

        def call(system, user):
            calls.append((system, user))
            return next(replies)

        self.assertEqual(batched(["\u6d4b\u8bd5"], call, "en"), ["This is a test."])
        self.assertEqual(len(calls), 2)
        self.assertIn("English", calls[1][0])

    def test_nonempty_source_with_empty_output_is_retried(self):
        replies = iter(('[""]', '["Translated"]'))
        calls = []

        def call(system, user):
            calls.append((system, user))
            return next(replies)

        self.assertEqual(batched(["\u6d4b\u8bd5"], call, "en"), ["Translated"])
        self.assertEqual(len(calls), 2)

    def test_short_wrong_chinese_target_output_is_retried(self):
        replies = iter(('["Hi"]', '["\u4f60\u597d"]'))
        self.assertEqual(
            batched(["Hello"], lambda _system, _user: next(replies), "zh-Hans"),
            ["\u4f60\u597d"],
        )

    def test_two_wrong_language_attempts_raise(self):
        replies = iter(('["\\u6d4b\\u8bd5"]', '["\\u4ecd\\u662f\\u4e2d\\u6587"]'))

        with self.assertRaises(TranslateError):
            batched(["\u6d4b\u8bd5"], lambda _system, _user: next(replies), "en")

    def test_retry_exception_and_bad_json_raise(self):
        def exception_call(_system, _user):
            exception_call.count += 1
            if exception_call.count == 1:
                return '["\\u6d4b\\u8bd5"]'
            raise RuntimeError("retry failed")

        exception_call.count = 0
        with self.assertRaises(TranslateError):
            batched(["\u6d4b\u8bd5"], exception_call, "en")

        malformed = iter(('["\\u6d4b\\u8bd5"]', "not-json"))
        with self.assertRaises(TranslateError):
            batched(["\u6d4b\u8bd5"], lambda _system, _user: next(malformed), "en")


if __name__ == "__main__":
    unittest.main()
