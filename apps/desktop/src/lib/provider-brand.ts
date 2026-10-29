export type ProviderBrand = 'openai'|'claude'|'google'|'googlecloud'|'azure'|'microsoft'|'nvidia'|'deepseek'|'ollama'|'bing'|'tencent'|'yandex'|'deepl'|'apple'|'windows'|'youdao'|'iciba'|'meta'|'qwen'|'mistral'|'zhipu'|'custom';
export function providerBrand(id:string,endpoint=''):ProviderBrand {
  const known:Record<string,ProviderBrand>={google_free:'google',google:'googlecloud',anthropic:'claude',microsoft:'microsoft',microsoft_free:'microsoft',bing_free:'bing',tencent_free:'tencent',yandex_free:'yandex',iciba_free:'iciba',nvidia:'nvidia',deepl:'deepl',azure:'azure',youdao:'youdao'};
  if(id!=='openai')return known[id]??'custom';
  try { const host=new URL(endpoint).hostname.toLowerCase();if(host==='api.deepseek.com')return 'deepseek';if(host.endsWith('.openai.azure.com'))return 'azure';if(host==='localhost'||host==='127.0.0.1')return new URL(endpoint).port==='11434'?'ollama':'custom'; } catch { /* Incomplete drafts retain the protocol brand. */ }
  return 'openai';
}
export function modelBrand(model:string,fallback:ProviderBrand):ProviderBrand {
  const patterns:Array<[RegExp,ProviderBrand]>=[[/claude/i,'claude'],[/deepseek/i,'deepseek'],[/gemini|gemma/i,'google'],[/llama|^meta[/-]/i,'meta'],[/qwen|qwq/i,'qwen'],[/mistral|mixtral|ministral/i,'mistral'],[/^glm|chatglm|zhipu/i,'zhipu'],[/(?:^|[/])(?:gpt-|chatgpt-|o[134](?:-|$))/i,'openai']];
  return patterns.find(([pattern])=>pattern.test(model))?.[1]??fallback;
}
