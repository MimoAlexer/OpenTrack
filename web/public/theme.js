// Apply the saved appearance before the first frame, including native startup.
try {
  const theme = localStorage.getItem('opentrack:theme');
  if (theme === 'light' || theme === 'dark') document.documentElement.dataset.theme = theme;
} catch { /* Follow the system when optional preference storage is unavailable. */ }
