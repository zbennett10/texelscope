// A small status note in the corner of the page being debugged, in its own shadow root so the
// app's styles leave it alone.

export interface ToastOptions {
  /** Shows the message as a problem. */
  problem?: boolean;
  /** A button next to the message, and what it does. */
  action?: { label: string; run(): void };
}

let host: HTMLElement | undefined;
let timer: ReturnType<typeof setTimeout> | undefined;

/** Shows a short message for a few seconds, or until the next one. */
export function toast(message: string, options: ToastOptions = {}): void {
  if (typeof document === 'undefined' || !document.body) return;
  if (!host) {
    host = document.createElement('texelscope-toast');
    const root = host.attachShadow({ mode: 'open' });
    root.innerHTML = `<style>
      div { position: fixed; right: 12px; bottom: 12px; z-index: 2147483647; max-width: 420px;
        padding: 8px 12px; border-radius: 8px; background: #111827; color: #f9fafb;
        font: 13px/1.45 system-ui, sans-serif; box-shadow: 0 4px 14px rgba(0, 0, 0, 0.3); }
      div.problem { background: #7f1d1d; }
      b { color: #f9a8d4; font-weight: 600; }
      button { margin-left: 8px; padding: 2px 10px; border: 0; border-radius: 6px; background: #f9a8d4;
        color: #111827; font: inherit; font-weight: 600; cursor: pointer; }
    </style><div><b>Texelscope</b> <span></span><button type="button"></button></div>`;
  }
  if (!host.isConnected) document.body.appendChild(host);
  const box = host.shadowRoot!.querySelector('div')!;
  box.className = options.problem ? 'problem' : '';
  box.querySelector('span')!.textContent = message;
  const button = box.querySelector('button')!;
  const action = options.action;
  button.hidden = !action;
  button.textContent = action?.label ?? '';
  button.onclick = action
    ? () => {
        host?.remove();
        action.run();
      }
    : null;
  clearTimeout(timer);
  timer = setTimeout(() => host?.remove(), action ? 30000 : options.problem ? 9000 : 4000);
}
