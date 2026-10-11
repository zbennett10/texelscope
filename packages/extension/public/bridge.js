// Tells the hook, which runs in the page's world without extension APIs, where the UI is.
const ui = chrome.runtime.getURL('ui/index.html');
document.addEventListener('texelscope:where', () => document.dispatchEvent(new CustomEvent('texelscope:ui', { detail: ui })));
