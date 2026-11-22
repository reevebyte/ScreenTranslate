<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { onDestroy, onMount, untrack } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { Dialog } from 'bits-ui';
  import {
    Globe,
    Monitor,
    ScanLine,
    RefreshCw,
    ArrowUpRight,
    ChevronDown,
    Check,
    Download,
    Trash2,
    RotateCcw,
    LoaderCircle,
    X,
  } from '$lib/icons';
  import {
    app,
    previewConfig,
    saveConfig,
    refreshCapabilities,
    act,
    registerFlush,
  } from '$lib/store.svelte';
  import { call, desktop } from '$lib/api';
  import { createAutosave } from '$lib/autosave';
  import { errorMessage } from '$lib/utils';
  import type { Config, LocalOcrStatus } from '$lib/generated';
  import ProviderIcon from '$lib/components/ProviderIcon.svelte';
  import { modelBrand, providerBrand } from '$lib/provider-brand';
  import type { ProviderBrand } from '$lib/provider-brand';
  import Switch from '$lib/components/ui/switch/switch.svelte';
  import SecretField from '$lib/components/SecretField.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import ServiceBrowser from '$lib/components/ServiceBrowser.svelte';
  import Choice from '$lib/components/ui/choice.svelte';

  let credentialField = $state<{ flush: () => Promise<boolean> }>();
  let draft = $state<Config>(structuredClone($state.snapshot(app.data!.config)));
  let editingText = $state(false);
  let selected = $state<Config['ocr']['engine']>(draft.ocr.engine);
  let last = untrack(() => JSON.stringify(draft));
  let confirmRemove = $state(false);
  let busyAction = $state(false);
  let localOcr = $state<LocalOcrStatus>({
    supported: true,
    state: 'not_installed',
    modelVersion: 'ppocrv5-mobile-v1',
    installedBytes: 0,
    downloadedBytes: 0,
    totalBytes: null,
    workerRunning: false,
    error: null,
  });

  const autosave = createAutosave<Config>(config => saveConfig(config, { quiet: true }));
  function queue() {
    const json = JSON.stringify(draft);
    if (json === last) return;
    last = json;
    const snapshot = structuredClone($state.snapshot(draft));
    previewConfig(snapshot);
    autosave.schedule(snapshot);
  }
  $effect(() => {
    JSON.stringify(draft);
    if (!editingText) untrack(queue);
  });
  // Keep draft synced if backend falls back the active engine (e.g. removing the active rapid model).
  $effect(() => {
    const currentEngine = app.data?.config.ocr.engine;
    if (currentEngine && currentEngine !== draft.ocr.engine && !app.configPending && !app.configSaving) {
      draft.ocr.engine = currentEngine;
      last = JSON.stringify(draft);
    }
  });

  const stopFlush = registerFlush(async () => {
    if (credentialField && !(await credentialField.flush())) return false;
    queue();
    return autosave.flush();
  });

  let unlistenLocalOcr: UnlistenFn | undefined;
  let destroyed = false;
  async function refreshLocalOcr() {
    try {
      const status = await call<LocalOcrStatus>('local_ocr_status');
      if (status && !destroyed) localOcr = status;
    } catch {
      /* Ignore status fetch errors during unmount or browser preview. */
    }
  }

  onMount(() => {
    void refreshLocalOcr();
    if (desktop) {
      void listen<LocalOcrStatus>('local-ocr-status', ({ payload }) => {
        if (!destroyed) localOcr = payload;
      }).then(fn => {
        if (destroyed) fn();
        else unlistenLocalOcr = fn;
      });
    }
  });

  onDestroy(() => {
    destroyed = true;
    unlistenLocalOcr?.();
    stopFlush();
    queue();
    void autosave.flush();
  });

  const engines = $derived([
    ['youdao', t('有道 OCR')],
    [
      'system',
      app.data?.capabilities.platform === 'macos'
        ? 'Apple Vision'
        : app.data?.capabilities.platform === 'linux'
          ? 'Tesseract'
          : 'Windows OCR',
    ],
    ['rapid', t('PP-OCRv5 本地')],
    ['azure', 'Azure Vision'],
    ['custom', t('自定义 OCR')],
  ] as const);

  let entries = $derived(
    engines.map(([id, name]) => ({
      id,
      name,
      icon: (id === 'system'
        ? app.data?.capabilities.platform === 'macos'
          ? 'apple'
          : app.data?.capabilities.platform === 'linux'
            ? 'custom'
            : 'windows'
        : id === 'rapid'
          ? 'local_ocr'
          : id === 'custom'
            ? 'custom'
            : id === 'azure'
              ? 'azure'
              : 'youdao') as ProviderBrand,
      group: id === 'system' || id === 'rapid' ? t('本机') : id === 'custom' ? t('自定义') : t('云端'),
      current: draft.ocr.engine === id,
    })),
  );

  const idleOptions = $derived([
    ['60', t('1 分钟')],
    ['180', t('3 分钟（默认）')],
    ['300', t('5 分钟')],
    ['0', t('不自动释放')],
  ] as const);

  const downloadPercent = $derived(
    localOcr.totalBytes && localOcr.totalBytes > 0
      ? Math.min(100, Math.round((localOcr.downloadedBytes / localOcr.totalBytes) * 100))
      : null,
  );

  const activateDisabled = $derived(
    (selected === 'rapid' && localOcr.state !== 'ready') ||
      (selected === 'custom' &&
        (!draft.ocr.custom.endpoint.trim() ||
          (draft.ocr.custom.protocol === 'openai' && !draft.ocr.custom.model.trim()))),
  );

  function formatMegabytes(bytes: number | null | undefined): string {
    if (!bytes || bytes <= 0) return '0 MB';
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  async function select(id: string) {
    if (credentialField && !(await credentialField.flush())) return;
    selected = id as Config['ocr']['engine'];
    if (selected === 'rapid') void refreshLocalOcr();
  }

  async function activate() {
    if (credentialField && !(await credentialField.flush())) return;
    draft.ocr.engine = selected;
    queue();
    await autosave.flush();
  }

  async function startDownload() {
    if (busyAction || localOcr.state === 'downloading') return;
    busyAction = true;
    localOcr = { ...localOcr, state: 'downloading', error: null };
    try {
      localOcr = await call<LocalOcrStatus>('download_local_ocr');
    } catch (error) {
      const cancelled =
        typeof error === 'object' && error !== null && 'code' in error && error.code === 'cancelled';
      const msg = errorMessage(error);
      await refreshLocalOcr();
      if (!cancelled && localOcr.state !== 'ready') {
        localOcr = { ...localOcr, state: 'error', error: localOcr.error || msg };
      }
    } finally {
      busyAction = false;
    }
  }

  async function cancelDownload() {
    try {
      localOcr = await call<LocalOcrStatus>('cancel_local_ocr_download');
    } catch {
      await refreshLocalOcr();
    }
  }

  async function removeModels() {
    if (busyAction) return;
    busyAction = true;
    try {
      localOcr = await call<LocalOcrStatus>('remove_local_ocr');
      confirmRemove = false;
    } catch (error) {
      localOcr = { ...localOcr, error: errorMessage(error) };
    } finally {
      busyAction = false;
    }
  }

  async function unloadWorker() {
    const status = await act(call<LocalOcrStatus>('unload_local_ocr_worker'));
    if (status) localOcr = status;
  }
</script>

<div
  class="service-layout"
  onfocusin={event => {
    if (event.target instanceof HTMLInputElement) editingText = true;
  }}
  onfocusout={() => {
    editingText = false;
    queue();
  }}
>
  <ServiceBrowser {entries} {selected} onselect={select} title={t('文字识别服务')}>
    {#snippet header()}<span class="service-protocol"
        >{selected === 'system'
          ? t('本机识别')
          : selected === 'rapid'
            ? t('离线高精度 · PP-OCRv5')
            : selected === 'custom'
              ? t('自定义接口')
              : selected === 'youdao'
                ? t('免密云端识别')
                : t('官方云端识别')}</span
      >{/snippet}

    {#snippet status()}
      {#if selected === 'rapid' && localOcr.error}
        <span class="connection-status error-text" title={localOcr.error}
          ><span>{localOcr.error}</span></span
        >
      {/if}
    {/snippet}

    {#snippet children()}
      {#if selected === 'youdao'}
        <div class="connection-line">
          <span class="setting-row-icon" aria-hidden="true"><Globe size={16} /></span><span
            >{t('选区截图发送至有道')}</span
          ><span class="service-mode">{t('无需密钥')}</span>
        </div>
      {:else if selected === 'system'}
        <div class="connection-line">
          <span class="setting-row-icon" aria-hidden="true"><Monitor size={16} /></span><span
            >{app.data?.capabilities.localOcr
              ? t('本机识别，无需上传')
              : t('需要安装识别语言包')}</span
          ><button
            class="icon-button"
            aria-label={t('重新检测 OCR')}
            title={t('重新检测')}
            onclick={refreshCapabilities}><RefreshCw size={17} /></button
          ><button
            class="icon-button"
            aria-label={t('OCR 安装指引')}
            title={t('安装指引')}
            onclick={() => act(call('open_help', { topic: 'ocr' }))}
            ><ArrowUpRight size={17} /></button
          >
        </div>
        <label class="field-label" for="ocr-languages">{t('识别语言')}</label><input
          id="ocr-languages"
          class="field"
          value={draft.ocr.languages.join(', ')}
          onchange={event =>
            (draft.ocr.languages = event.currentTarget.value
              .split(',')
              .map(value => value.trim())
              .filter(Boolean))}
        />
      {:else if selected === 'rapid'}
        <div class="provider-form">
          {#if !localOcr.supported}
            <div class="connection-line">
              <span class="setting-row-icon" aria-hidden="true"><Monitor size={16} /></span>
              <span>{t('当前系统架构暂不支持内置 PP-OCRv5 本地引擎')}</span>
            </div>
          {:else if localOcr.state === 'ready'}
            <div class="connection-line">
              <span class="setting-row-icon" aria-hidden="true"><Monitor size={16} /></span>
              <span
                >{t(
                  '已就绪 · 占用 {0} · 不上传截图',
                  formatMegabytes(localOcr.installedBytes),
                )}</span
              >
              {#if localOcr.workerRunning}
                <button
                  class="icon-button"
                  aria-label={t('立即释放 OCR 内存')}
                  title={t('立即释放内存')}
                  onclick={unloadWorker}><RotateCcw size={16} /></button
                >
              {/if}
              <button
                class="icon-button"
                aria-label={t('删除本地 OCR 模型')}
                title={t('删除模型')}
                onclick={() => (confirmRemove = true)}><Trash2 size={16} /></button
              >
            </div>
          {:else if localOcr.state === 'downloading'}
            <div class="local-ocr-panel">
              <div class="local-ocr-panel-head">
                <span class="setting-row-icon" aria-hidden="true"
                  ><LoaderCircle size={16} class="spin" /></span
                >
                <div class="local-ocr-panel-copy">
                  <strong
                    >{t(
                      '正在下载模型与运行时 {0}',
                      downloadPercent === null ? '…' : `${downloadPercent}%`,
                    )}</strong
                  >
                  <span
                    >{formatMegabytes(localOcr.downloadedBytes)} / {formatMegabytes(
                      localOcr.totalBytes,
                    )}</span
                  >
                </div>
                <Button variant="outline" size="sm" onclick={cancelDownload}>{t('取消')}</Button>
              </div>
              <div
                class="local-ocr-progress"
                role="progressbar"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={downloadPercent ?? 0}
              >
                <div
                  class="local-ocr-progress-fill"
                  style:width={`${downloadPercent ?? 12}%`}
                ></div>
              </div>
            </div>
          {:else}
            <div class="local-ocr-panel">
              <div class="local-ocr-panel-head">
                <span class="setting-row-icon" aria-hidden="true"><Download size={16} /></span>
                <div class="local-ocr-panel-copy">
                  <strong>{t('按需下载离线高精度模型（中 / 英 / 日）')}</strong>
                  <span
                    >{t(
                      '约 {0}，仅下载到本机数据目录；空闲时自动释放推理内存。',
                      formatMegabytes(localOcr.totalBytes ?? 37_600_000),
                    )}</span
                  >
                </div>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={busyAction}
                  onclick={startDownload}
                >
                  {localOcr.state === 'error' ? t('重试下载') : t('下载模型')}
                </Button>
              </div>
            </div>
            {#if app.data?.capabilities.platform === 'macos'}
              <p class="local-ocr-hint">
                {t('macOS 系统自带的 Apple Vision 已具备高准确率且零磁盘占用，按需下载即可。')}
              </p>
            {/if}
          {/if}

          <label class="field-label" for="rapid-idle-unload">{t('空闲自动释放内存')}</label>
          <Choice
            id="rapid-idle-unload"
            class="field"
            label={t('空闲自动释放内存')}
            value={String(draft.ocr.rapid.idleUnloadSecs)}
            options={idleOptions}
            onchange={v => (draft.ocr.rapid.idleUnloadSecs = Number(v))}
          />
          <p class="local-ocr-hint">
            {t('推理在独立子进程中运行，空闲超时或切换引擎后由系统彻底回收内存。')}
          </p>
        </div>
      {:else if selected === 'custom'}
        <div class="provider-form">
          <label class="field-label" for="ocr-protocol">{t('接口协议')}</label><Choice
            id="ocr-protocol"
            class="field"
            label={t('接口协议')}
            value={draft.ocr.custom.protocol}
            options={[
              ['openai', t('OpenAI 兼容视觉模型')],
              ['http_json', t('通用 HTTP / JSON')],
            ]}
            onchange={v =>
              (draft.ocr.custom.protocol = v as Config['ocr']['custom']['protocol'])}
          />
          <label class="field-label" for="custom-ocr-key">{t('API Key · 可选')}</label><SecretField
            bind:this={credentialField}
            id="custom-ocr-key"
            name="custom_ocr"
            label="OCR API Key"
          />
          {#if draft.ocr.custom.protocol === 'openai'}<label class="field-label" for="ocr-model"
              >{t('视觉模型')}</label
            ><div class="model-picker model-manual">
              <span class="model-input-brand"
                ><ProviderIcon
                  brand={modelBrand(
                    draft.ocr.custom.model,
                    providerBrand('openai', draft.ocr.custom.endpoint),
                  )}
                /></span
              ><input
                id="ocr-model"
                class="field"
                bind:value={draft.ocr.custom.model}
                placeholder={t('例如 gpt-4.1-mini')}
                spellcheck="false"
              />
            </div>{/if}
          <details class="service-advanced" open={!draft.ocr.custom.endpoint}>
            <summary><ChevronDown size={15} /><span>{t('接口设置')}</span></summary>
            <div class="service-advanced-body">
              <label class="field-label" for="ocr-endpoint">{t('服务地址')}</label><input
                id="ocr-endpoint"
                class="field"
                bind:value={draft.ocr.custom.endpoint}
                placeholder={draft.ocr.custom.protocol === 'openai'
                  ? 'https://api.example.com/v1'
                  : 'https://api.example.com/ocr'}
                spellcheck="false"
              />
              <details class="protocol-help">
                <summary>{t('接口格式')}</summary>{#if draft.ocr.custom.protocol === 'http_json'}<p
                    >{t(
                      'POST JSON，image 为 PNG Base64，另含 format、width、height、languages。',
                    )}</p
                  ><pre>{'{"blocks":[{"text":"Hello","bounds":{"x":0,"y":0,"width":100,"height":20}}]}'}</pre>{:else}<p
                    >{t('使用 Chat Completions 图像输入。服务需支持视觉识别与 JSON 输出。')}</p
                  >{/if}<p>
                  {t('坐标单位为请求图片像素。也接受')}
                  {t('{"text":"识别结果"}')}{t('，作为整图文字块。')}
                </p>
              </details>
            </div>
          </details>
          <div class="service-disclosure">
            <Globe size={16} /><span>{t('选区截图发送至填写的服务。')}</span>
          </div>
        </div>
      {:else}
        <div class="provider-form">
          <label class="field-label" for="azure-endpoint">{t('服务地址')}</label><input
            id="azure-endpoint"
            class="field"
            bind:value={draft.ocr.azureEndpoint}
            placeholder={t('https://你的资源.cognitiveservices.azure.com')}
          />
          <label class="field-label" for="azure-key">{t('Vision 密钥')}</label><SecretField
            bind:this={credentialField}
            id="azure-key"
            name="azure_ocr"
            label={t('Vision 密钥')}
          />
          <div class="service-disclosure">
            <Globe size={16} /><span>{t('选区截图发送至 Azure。')}</span>
          </div>
        </div>
      {/if}
      <div class="ocr-processing-option">
        <div class="setting-row-lead">
          <span class="setting-row-icon" aria-hidden="true"><ScanLine size={16} /></span><label
            for="ocr-upscale">{t('放大小图后识别')}</label
          >
        </div>
        <Switch id="ocr-upscale" bind:checked={draft.ocr.upscale} />
      </div>
    {/snippet}

    {#snippet footer()}
      {#if selected === draft.ocr.engine}
        <span class="service-current"><Check size={16} />{t('当前使用中')}</span>
      {:else}
        <Button disabled={activateDisabled} onclick={activate}>{t('设为当前')}</Button>
      {/if}
    {/snippet}
  </ServiceBrowser>
</div>

<Dialog.Root bind:open={confirmRemove}>
  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay" />
    <Dialog.Content
      class="reading-dialog update-dialog"
      onEscapeKeydown={event => {
        if (busyAction) event.preventDefault();
      }}
      onInteractOutside={event => {
        if (busyAction) event.preventDefault();
      }}
    >
      <div class="dialog-heading">
        <Dialog.Title>{t('删除本地 OCR 模型')}</Dialog.Title>
        <Dialog.Close class="icon-button" disabled={busyAction} aria-label={t('关闭')}
          ><X size={18} /></Dialog.Close
        >
      </div>
      <Dialog.Description
        >{draft.ocr.engine === 'rapid'
          ? t('删除后将释放磁盘空间，并自动切换回其他可用的识别引擎。之后随时可重新下载。')
          : t('删除后将清理本机模型与运行时文件，之后随时可重新下载。')}</Dialog.Description
      >
      <div class="dialog-actions">
        <Dialog.Close class="button button-outline button-size-default" disabled={busyAction}
          >{t('取消')}</Dialog.Close
        >
        <Button disabled={busyAction} onclick={removeModels}>{t('删除模型')}</Button>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
