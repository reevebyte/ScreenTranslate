import { mount } from 'svelte';
import { initializeLocale } from '$lib/i18n.svelte';
import CaptureOverlay from './views/CaptureOverlay.svelte';
import './selection.css';

document.body.dataset.window = 'selection';
window.addEventListener('contextmenu', event => event.preventDefault(), {
  capture: true,
});

initializeLocale();
mount(CaptureOverlay, { target: document.getElementById('app')! });
