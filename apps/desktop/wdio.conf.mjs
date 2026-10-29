import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';

// Deterministic localhost translation service. Desktop E2E never needs API keys or paid services.
const server = createServer(async (request, response) => {
  if (request.method === 'GET' && request.url === '/v1/models') {
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(JSON.stringify({data:[{id:'fixture-model'},{id:'fixture-vision'}]})); return;
  }
  const chunks = [];
  for await (const chunk of request) chunks.push(chunk);
  try {
    const body = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    if(request.url === '/ocr') {
      response.writeHead(200,{'Content-Type':'application/json'});
      response.end(JSON.stringify({blocks:[{text:'Custom OCR fixture',bounds:{x:20,y:20,width:Math.min(400,body.width-20),height:Math.min(50,body.height-20)}}]}));return;
    }
    const fixtureTranslation = text => {
      if (text.includes('已改好并构建')) return 'Updated and built alpha.4.';
      if (text.includes('去掉顶部标志')) return 'Removed the logo, extra icons, gradients and repeated captions.';
      if (text.includes('截图译文保留')) return 'Translations stay in place without switching pages.';
      if (text.includes('拖动手柄和操作栏')) return 'Drag and action controls sit outside the selection, at the bottom right.';
      if (text.includes('文字翻译页')) return 'Text translation:';
      return `本地测试：${text}`;
    };
    const items = JSON.parse(body.messages[1].content).map(item => ({ id: item.id, text: fixtureTranslation(item.text) }));
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(JSON.stringify({ choices: [{ message: { content: JSON.stringify({ items }) } }] }));
  } catch {
    response.writeHead(400); response.end();
  }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const endpoint = `http://127.0.0.1:${server.address().port}/v1`;
const appBinaryPath = fileURLToPath(new URL(`../../target/debug/screentranslate${process.platform === 'win32' ? '.exe' : ''}`, import.meta.url));

export const config = {
  runner: 'local',
  specs: ['./tests/e2e/desktop.mjs'],
  maxInstances: 1,
  capabilities: [{ browserName: 'tauri', 'tauri:options': { application: appBinaryPath } }],
  services: [['tauri', { appBinaryPath, driverProvider: 'embedded' }]],
  framework: 'mocha',
  reporters: ['spec'],
  logLevel: 'warn',
  waitforTimeout: 15000,
  connectionRetryCount: 1,
  mochaOpts: { timeout: 90000 },
  before: async () => {
    await browser.$('.app-shell').waitForDisplayed();
    await browser.execute(async()=>{document.querySelectorAll('.rail-main .rail-link')[1]?.click();await new Promise(resolve=>setTimeout(resolve,350));});
    await browser.execute(async()=>{localStorage.setItem('screentranslate-ui-language','zh-CN');await window.__TAURI__.event.emit('ui-locale-changed',{language:'zh-CN'});});
    await browser.tauri.execute(async ({ core }, endpoint) => {
      const snapshot = await core.invoke('bootstrap');
      const config = snapshot.config;
      config.onboardingComplete = true;
      config.autoTranslate = false;
      config.appearance.theme = 'light';
      config.appearance.fontScale = 1;
      config.ocr.engine = 'system';
      config.ocr.custom.protocol = 'http_json';
      config.ocr.custom.endpoint = endpoint.replace(/\/v1$/, '') + '/ocr';
      config.provider = 'openai';
      config.providers.openai.baseUrl = endpoint;
      config.providers.openai.model = 'fixture-model';
      await core.invoke('save_config', { config });
    }, endpoint);
    const close = await browser.$('button[aria-label="稍后配置"]');
    if (await close.isExisting()) await close.click();
  },
  after: async () => { server.close(); },
  onComplete: () => { server.close(); },
};
