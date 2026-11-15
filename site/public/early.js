// Picks the theme before the first paint (a file, not inline: the Content-Security-Policy runs
// only the site's own scripts, TODO 6.4.1).
// Pick the theme before the first paint: the saved one, else the default terminal of this OS.
(() => {
  const platform = navigator.userAgentData?.platform || navigator.platform || '';
  const os = /win/i.test(platform) ? 'windows'
    : /mac/i.test(platform) && navigator.maxTouchPoints < 2 ? 'macos'
    : /linux/i.test(platform) && !/android/i.test(navigator.userAgent) ? 'linux'
    : '';
  let saved = null;
  try { saved = localStorage.getItem('sozo-theme'); } catch {}
  const root = document.documentElement;
  root.dataset.os = os;
  root.dataset.theme = saved || { windows: 'campbell', macos: 'iterm2', linux: 'ubuntu' }[os] || 'campbell';
  // The moving parts wait hidden for the motion script; if it never runs, they show anyway.
  if (matchMedia('(prefers-reduced-motion: no-preference)').matches) {
    root.dataset.motion = 'wait';
    setTimeout(() => {
      if (root.dataset.motion === 'wait') root.dataset.motion = 'off';
    }, 3000);
  }
})();
