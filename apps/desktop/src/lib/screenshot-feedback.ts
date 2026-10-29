import { t } from './i18n.svelte';
import type { AppError, TranslationSession } from './generated';

export function extractedText(session?: Pick<TranslationSession, 'blocks' | 'input'>) {
  return session?.blocks.map(block => block.text.trim()).filter(Boolean).join('\n\n') || session?.input.trim() || '';
}

/** Keep small screenshot notices short; the toolbar exposes the full error. */
export function screenshotNotice(error: Pick<AppError, 'code' | 'message'>) {
  const text = error.message;
  if (/没有识别|未识别|没有文字/.test(text)) return t("未识别到文字");
  if (/HTTPS|服务地址|URL|配置|密钥|模型/.test(text)) return t("请检查服务配置");
  if (/401|认证|授权/.test(text)) return t("服务认证失败");
  if (/429|限流|频繁/.test(text)) return t("请求过于频繁");
  if (/超时/.test(text)) return t("处理超时，请重试");
  if (/连接|网络/.test(text)) return t("连接失败，请重试");
  return t("无法翻译此图片");
}
