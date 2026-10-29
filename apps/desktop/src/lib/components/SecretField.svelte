<script lang="ts">
  import { t } from '$lib/i18n.svelte';
  import { onDestroy } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { app } from '$lib/store.svelte';
  import { call } from '$lib/api';
  import { errorMessage } from '$lib/utils';
  import { Eye, EyeOff, Trash2, LoaderCircle } from '$lib/icons';
  let { name, id, label = t("密钥"), persistent = true, placeholder = t("输入密钥"), onchange }:
    { name: string; id: string; label?: string; persistent?: boolean; placeholder?: string; onchange?: () => void } = $props();
  let draft = $state('');
  let stored = $state('');
  let visible = $state(false);
  let loading = $state(false);
  let error = $state('');
  let saving = $state(false);
  let flight: Promise<boolean> | null = null;
  let exists = $derived(!!app.data?.credentials[name]);

  export async function flush(): Promise<boolean> {
    if (flight) return flight;
    if (!draft.trim()) return true;
    const value = draft;
    const credential = name;
    saving = true;
    flight = (async () => {
      try {
        const durable = await call<boolean>('save_secret', { name: credential, secret: value, persistent });
        stored = value; draft = ''; error = '';
        if (app.data) app.data.credentials[credential] = true;
        if (persistent && !durable) toast.warning(t("密钥仅保留在本次会话"));
        return true;
      } catch (cause) { error = errorMessage(cause); toast.error(error); return false; }
      finally { saving = false; flight = null; }
    })();
    return flight;
  }
  async function toggle() {
    if (!visible && !draft && !stored && exists) {
      loading = true;
      try { stored = await call<string | null>('read_secret', { name }) || ''; error = ''; }
      catch (cause) { error = errorMessage(cause); return; }
      finally { loading = false; }
    }
    visible = !visible;
  }
  async function remove() {
    if (!(await flush())) return;
    try { await call('delete_secret', { name }); stored = ''; draft = ''; visible = false; if (app.data) app.data.credentials[name] = false; onchange?.(); }
    catch (cause) { error = errorMessage(cause); }
  }
  onDestroy(() => { void flush(); });
</script>

<div class="secret-control">
  <div class="secret-field">
    <input {id} aria-label={label} class="field" type={visible ? 'text' : 'password'} value={draft || stored} oninput={event => { draft = event.currentTarget.value; onchange?.(); }} onblur={() => void flush()} disabled={saving} placeholder={exists ? '••••••••••••' : placeholder} autocomplete="off" spellcheck="false"/>
    <button type="button" class="icon-button" disabled={loading} aria-label={visible ? t("隐藏{0}",label) : t("查看{0}",label)} onclick={toggle}>{#if loading}<LoaderCircle size={17} class="spin"/>{:else if visible}<EyeOff size={17}/>{:else}<Eye size={17}/>{/if}</button>
    {#if exists}<button type="button" class="icon-button" aria-label={t("删除{0}",label)} disabled={saving} onclick={remove}><Trash2 size={16}/></button>{/if}
  </div>
  {#if error}<div class="autosave-error" role="alert"><span>{error}</span><button onclick={() => draft ? flush() : toggle()}>{t("重试")}</button></div>{/if}
</div>
