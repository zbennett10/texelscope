// Starts the hook on a site when the user turns Texelscope on for it, and stops it when they
// turn it off. Local dev servers have it from the manifest.

/** The two content scripts for a site, named after its match pattern. */
function scriptsFor(origin) {
  const id = origin.replace(/[^a-z0-9]+/gi, '-');
  return [
    { id: `hook-${id}`, matches: [origin], js: ['hook.js'], runAt: 'document_start', world: 'MAIN', allFrames: true },
    { id: `bridge-${id}`, matches: [origin], js: ['bridge.js'], runAt: 'document_start', allFrames: true },
  ];
}

chrome.permissions.onAdded.addListener(async ({ origins = [] }) => {
  const registered = new Set((await chrome.scripting.getRegisteredContentScripts()).map((script) => script.id));
  const fresh = origins.flatMap(scriptsFor).filter((script) => !registered.has(script.id));
  if (fresh.length > 0) await chrome.scripting.registerContentScripts(fresh);
  // The hook has to start before the page's own scripts, so reload the pages already open.
  if (origins.length > 0) for (const tab of await chrome.tabs.query({ url: origins })) await chrome.tabs.reload(tab.id);
});

chrome.permissions.onRemoved.addListener(async ({ origins = [] }) => {
  const ids = origins.flatMap(scriptsFor).map((script) => script.id);
  const registered = (await chrome.scripting.getRegisteredContentScripts({ ids })).map((script) => script.id);
  if (registered.length > 0) await chrome.scripting.unregisterContentScripts({ ids: registered });
});
