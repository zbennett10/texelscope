// Runs in the page's own world before its scripts, so the hooks see every shader the app
// makes. The extension's other content script knows where the bundled UI is.

import { startAuto } from '../../sdk/src/auto.js';

/** Asks the bridge script for the UI's address, which only extension scripts can look up. */
function uiUrl(): string | undefined {
  let url: string | undefined;
  const answer = (event: Event) => {
    url = (event as CustomEvent<string>).detail;
  };
  document.addEventListener('texelscope:ui', answer);
  document.dispatchEvent(new Event('texelscope:where'));
  document.removeEventListener('texelscope:ui', answer);
  return url;
}

startAuto({ ui: uiUrl });
