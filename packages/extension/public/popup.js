// Turns Texelscope on or off for the site in the current tab. Local dev servers always have it.

const status = document.getElementById('status');
const toggle = document.getElementById('toggle');

const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
const url = tab?.url ? new URL(tab.url) : undefined;

if (!url || (url.protocol !== 'http:' && url.protocol !== 'https:')) {
  status.textContent = 'Open your app in this tab first.';
} else if (url.hostname === 'localhost' || url.hostname === '127.0.0.1') {
  status.textContent = `On for ${url.host}, as for every local dev server. If the page was open before Texelscope was installed, reload it.`;
} else {
  const site = `${url.protocol}//${url.hostname}/*`;
  const on = await chrome.permissions.contains({ origins: [site] });
  status.textContent = on ? `On for ${url.hostname}.` : `Off for ${url.hostname}. Turning it on reloads the page.`;
  toggle.textContent = on ? `Turn off for ${url.hostname}` : `Turn on for ${url.hostname}`;
  toggle.hidden = false;
  toggle.addEventListener('click', async () => {
    // The background script starts or stops the hook as the permission changes.
    if (on) {
      if (await chrome.permissions.remove({ origins: [site] })) await chrome.tabs.reload(tab.id);
    } else {
      await chrome.permissions.request({ origins: [site] });
    }
    window.close();
  });
}
