if (typeof importScripts === "function" && !globalThis.WOLSync) {
  // `lib/oauth.generated.js` is produced by `node scripts/gen-oauth.js` at build
  // time. It can be absent in a plain checkout, so load it defensively.
  for (const scriptPath of ["lib/oauth.generated.js", "lib/sync.js"]) {
    try {
      importScripts(scriptPath);
    } catch (err) {
      console.warn(`[WOL Highlights Background] Could not load ${scriptPath}:`, err);
    }
  }
}

const ext = typeof browser !== "undefined" ? browser : chrome;

ext.action.onClicked.addListener(async () => {
  const pageUrl = ext.runtime.getURL("popup.html");
  try {
    const tabs = await ext.tabs.query({});
    const existingTab = tabs.find((t) => t.url === pageUrl);

    if (existingTab && existingTab.id !== undefined) {
      await ext.tabs.update(existingTab.id, { active: true });
      if (existingTab.windowId !== undefined) {
        await ext.windows.update(existingTab.windowId, { focused: true });
      }
    } else {
      await ext.tabs.create({ url: pageUrl });
    }
  } catch (err) {
    console.error("[WOL Highlights Background] Failed to open/focus manager tab:", err);
    ext.tabs.create({ url: pageUrl });
  }
});

async function broadcastReapplyToWolTabs() {
  try {
    const tabs = await ext.tabs.query({ url: "https://wol.jw.org/*" });
    for (const tab of tabs) {
      if (tab?.id) {
        ext.tabs.sendMessage(tab.id, { type: "wol-reapply" }).catch(() => {});
      }
    }
  } catch (_) {
    // Ignore tab broadcast errors
  }
}

async function runSync(trigger = "manual") {
  console.info("[WOL Highlights Sync] Trigger:", trigger);
  if (!globalThis.WOLSync) return null;
  try {
    const status = await globalThis.WOLSync.syncNow(ext);
    if (status?.changedLocal) {
      await broadcastReapplyToWolTabs();
    }
    return status;
  } catch (err) {
    console.warn("[WOL Highlights Sync] Background sync failed:", err);
    throw err;
  }
}

let debounceTimer = null;
function scheduleDebouncedSync() {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => {
    debounceTimer = null;
    runSync("edit").catch(() => {});
  }, 3000);
}

ext.storage.onChanged.addListener(async (changes, area) => {
  if (area !== "local" || !changes.wolMarks) return;
  try {
    const stored = await ext.storage.local.get("wolSync");
    const syncState = stored?.wolSync;
    if (!syncState || !syncState.provider || syncState.provider === "none") return;

    // Skip if this write came from syncNow() itself
    if (syncState._internalWriteAt && Date.now() - syncState._internalWriteAt < 2000) {
      return;
    }

    syncState.localUpdatedAt = new Date().toISOString();
    await ext.storage.local.set({ wolSync: syncState });
    scheduleDebouncedSync();
  } catch (err) {
    console.warn("[WOL Highlights Sync] onChanged handler error:", err);
  }
});

if (ext.alarms) {
  ext.alarms.create("wol-cloud-sync", { periodInMinutes: 5 });
  ext.alarms.onAlarm.addListener((alarm) => {
    if (alarm.name === "wol-cloud-sync") {
      runSync("alarm").catch(() => {});
    }
  });
}

if (ext.runtime.onStartup) {
  ext.runtime.onStartup.addListener(() => {
    runSync("startup").catch(() => {});
  });
}

// Also run once when background loads
setTimeout(() => {
  runSync("background load").catch(() => {});
}, 2000);

ext.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (!msg || typeof msg.type !== "string" || !msg.type.startsWith("wol-sync-")) {
    return false;
  }

  (async () => {
    const WOLSync = globalThis.WOLSync;
    if (!WOLSync) throw new Error("Sync module not loaded.");

    if (msg.type === "wol-sync-status") {
      const state = await WOLSync.getOrInitSyncState(ext);
      return WOLSync.buildStatus(state, false, false);
    }
    if (msg.type === "wol-sync-now") {
      return await runSync();
    }
    if (msg.type === "wol-sync-google") {
      const status = await WOLSync.connectGoogleDrive(ext);
      if (status?.changedLocal) await broadcastReapplyToWolTabs();
      return status;
    }
    if (msg.type === "wol-sync-nextcloud") {
      const status = await WOLSync.connectNextcloud(ext, msg.payload || {});
      if (status?.changedLocal) await broadcastReapplyToWolTabs();
      return status;
    }
    if (msg.type === "wol-sync-nextcloud-browser") {
      const status = await WOLSync.connectNextcloudBrowser(ext, msg.payload || {});
      if (status?.changedLocal) await broadcastReapplyToWolTabs();
      return status;
    }
    if (msg.type === "wol-sync-disconnect") {
      return await WOLSync.disconnectSync(ext);
    }
    throw new Error(`Unknown sync message: ${msg.type}`);
  })()
    .then((data) => sendResponse({ ok: true, data }))
    .catch((err) => sendResponse({ ok: false, error: err?.message || String(err) }));

  return true;
});
