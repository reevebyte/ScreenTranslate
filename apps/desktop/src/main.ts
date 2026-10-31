import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';
import './windows.css';

document.body.dataset.window = new URLSearchParams(location.search).get('window') || 'main';
window.addEventListener('contextmenu', event => event.preventDefault(), { capture: true });

if (import.meta.env.VITE_DESKTOP_E2E === 'true') void import('@wdio/tauri-plugin');

mount(App, { target: document.getElementById('app')! });
