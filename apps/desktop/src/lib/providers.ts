import type { ProviderId } from './generated';

export const providers: { id: ProviderId; name: string; mark: string; description: string; color: string; unofficial?: boolean; legacy?: boolean }[] = [
  { id: 'google_free', name: 'Google 网页翻译', mark: 'G', description: '免密使用 · 非官方接口', color: '#4285f4', unofficial: true },
  { id: 'openai', name: 'OpenAI 兼容', mark: 'AI', description: '自定义服务 / Ollama', color: '#16865a' },
  { id: 'microsoft', name: 'Microsoft Translator', mark: 'M', description: 'Azure 官方翻译 API', color: '#0085d5' },
  { id: 'deepl', name: 'DeepL', mark: 'D', description: '专业的语言翻译服务', color: '#125b8e' },
  { id: 'google', name: 'Google Cloud', mark: 'G', description: 'Google Cloud 官方 API', color: '#e8a514' },
  { id: 'anthropic', name: 'Anthropic', mark: 'A', description: 'Claude Messages API', color: '#bc7957' },
  { id: 'nvidia', name: 'NVIDIA NIM', mark: 'N', description: 'NVIDIA 模型推理服务', color: '#6a9a17' },
  { id: 'bing_free', name: 'Bing 网页翻译', mark: 'B', description: '免密接口', color: '#288b89', unofficial: true, legacy: true },
  { id: 'microsoft_free', name: 'Microsoft 移动接口', mark: 'M', description: '免密接口', color: '#0085d5', unofficial: true, legacy: true },
  { id: 'tencent_free', name: '腾讯交互翻译', mark: 'T', description: '免密接口', color: '#337ce1', unofficial: true, legacy: true },
  { id: 'yandex_free', name: 'Yandex 移动接口', mark: 'Y', description: '免密接口', color: '#d75444', unofficial: true, legacy: true },
  { id: 'iciba_free', name: '金山词霸', mark: 'K', description: '免密接口', color: '#e99c28', unofficial: true, legacy: true },
];

export const providerInfo = (id: ProviderId) => providers.find(provider => provider.id === id)!;

