try {
  const params = new URLSearchParams(location.search);
  const isSelection = location.pathname.endsWith('/selection.html');
  const win = isSelection ? 'selection' : (params.get('window') || 'main');
  if (win !== 'main') {
    document.documentElement.dataset.transparent = 'true';
    document.documentElement.dataset.window = win;
    document.documentElement.style.background = 'transparent';
  }
  const theme = params.get('theme') || localStorage.getItem('screentranslate-theme') || 'system';
  const dark = theme === 'dark' || (theme === 'system' && matchMedia('(prefers-color-scheme: dark)').matches);
  document.documentElement.classList.toggle('dark', dark);
} catch {}
