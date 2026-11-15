const ext = typeof browser !== "undefined" ? browser : chrome;

const COLOR_NAMES = {
  1: "Yellow",
  2: "Green",
  3: "Blue",
  4: "Red",
  5: "Orange",
  6: "Purple",
};

const COLOR_CSS = {
  1: "#ffe36b",
  2: "#b6e89a",
  3: "#a9d4fb",
  4: "#f7b4c4",
  5: "#ffc48a",
  6: "#d8c0f8",
};

const fileInput = document.querySelector("#file");
const loaded = document.querySelector("#loaded");
const fileName = document.querySelector("#file-name");
const counts = document.querySelector("#counts");
const colors = document.querySelector("#colors");
const enabledInput = document.querySelector("#enabled");
const removeButton = document.querySelector("#remove");
const exportButton = document.querySelector("#export-json");
const pageStatus = document.querySelector("#page-status");
const message = document.querySelector("#message");

function setMessage(text, isError = false) {
  message.textContent = text || "";
  message.classList.toggle("error", Boolean(isError));
}

function formatWhen(iso) {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleString();
}

async function currentIndex() {
  try {
    const all = await ext.storage.local.get(null);
    console.log("[WOL Highlights Popup] Storage check. Keys in storage.local:", Object.keys(all));
    const stored = all.wolMarks;
    if (!stored) {
      console.log("[WOL Highlights Popup] No 'wolMarks' key found in storage.local.");
      return null;
    }
    if (stored.version !== 1) {
      console.warn("[WOL Highlights Popup] 'wolMarks' version mismatch. Expected 1, found:", stored.version);
      return null;
    }
    console.log("[WOL Highlights Popup] Found valid 'wolMarks':", {
      fileName: stored.fileName,
      highlights: stored.highlightCount,
      documents: stored.documentCount,
      bibleChapters: stored.bibleChapterCount,
      enabled: stored.enabled !== false,
      loadedAt: stored.loadedAt,
    });
    return stored;
  } catch (err) {
    console.error("[WOL Highlights Popup] Failed to read from ext.storage.local:", err);
    return null;
  }
}

function renderIndex(index) {
  if (!index) {
    loaded.hidden = true;
    return;
  }
  loaded.hidden = false;
  const when = formatWhen(index.loadedAt);
  fileName.textContent = when ? `${index.fileName} · ${when}` : index.fileName;
  const places = [];
  if (index.documentCount) {
    places.push(`${index.documentCount} article${index.documentCount === 1 ? "" : "s"}`);
  }
  if (index.bibleChapterCount) {
    places.push(`${index.bibleChapterCount} Bible chapter${index.bibleChapterCount === 1 ? "" : "s"}`);
  }
  const noteInfo = index.noteCount ? ` · ${index.noteCount.toLocaleString()} note${index.noteCount === 1 ? "" : "s"}` : "";
  counts.textContent = `${index.highlightCount.toLocaleString()} highlight${index.highlightCount === 1 ? "" : "s"}${noteInfo}`
    + (places.length ? ` in ${places.join(" and ")}` : "");
  colors.replaceChildren();
  for (const [color, count] of Object.entries(index.colorCounts || {})) {
    const item = document.createElement("li");
    const swatch = document.createElement("span");
    swatch.className = "swatch";
    swatch.style.background = COLOR_CSS[color] || "#e6e6e6";
    item.append(swatch, `${COLOR_NAMES[color] || "Other"} ${count}`);
    colors.append(item);
  }
  enabledInput.checked = index.enabled !== false;
}

function describePage(status) {
  if (!status) return "Open an article on wol.jw.org.";
  if (!status.enabled && status.hasBackup) return "Highlights and notes are hidden.";
  const bits = [];
  if (status.applied > 0) {
    bits.push(`${status.applied} highlight${status.applied === 1 ? "" : "s"} shown`);
  }
  if (status.notesApplied > 0) {
    bits.push(`${status.notesApplied} note${status.notesApplied === 1 ? "" : "s"} shown`);
  }
  if (status.textareasEnabled > 0 && status.notesApplied === 0) {
    bits.push(`${status.textareasEnabled} note field${status.textareasEnabled === 1 ? "" : "s"} enabled`);
  }
  if (status.missing) bits.push(`${status.missing} not on this page`);
  if (status.skipped) bits.push(`${status.skipped} did not match the page text`);
  if (bits.length) return bits.join(". ") + ".";
  return "Select text to highlight, or type personal notes in paragraph textareas.";
}

async function refreshPage() {
  console.log("[WOL Highlights Manager] refreshPage() querying wol.jw.org tabs...");
  let tabs = [];
  try {
    tabs = await ext.tabs.query({ url: "https://wol.jw.org/*" });
  } catch (err) {
    console.error("[WOL Highlights Manager] Failed to query wol.jw.org tabs:", err);
  }

  if (!tabs || tabs.length === 0) {
    pageStatus.textContent = "No open article on wol.jw.org detected.";
    console.log("[WOL Highlights Manager] No tabs open on wol.jw.org.");
    return;
  }

  // Broadcast reapply to all open wol.jw.org tabs
  let primaryStatus = null;
  let activeWOLTab = tabs.find((t) => t.active) || tabs[0];

  for (const tab of tabs) {
    if (!tab?.id) continue;
    try {
      console.log("[WOL Highlights Manager] Sending 'wol-reapply' to tab ID " + tab.id + " (" + tab.url + ")...");
      const status = await ext.tabs.sendMessage(tab.id, { type: "wol-reapply" });
      if (tab.id === activeWOLTab.id) {
        primaryStatus = status;
      }
    } catch (err) {
      console.warn("[WOL Highlights Manager] Could not communicate with content script on tab " + tab.id + ":", err);
    }
  }

  if (primaryStatus) {
    const countInfo = tabs.length > 1 ? ` (${tabs.length} tabs updated)` : "";
    pageStatus.textContent = describePage(primaryStatus) + countInfo;
  } else {
    pageStatus.textContent = `Found ${tabs.length} open wol.jw.org tab${tabs.length > 1 ? "s" : ""}. Reload the article tab if highlights do not appear immediately.`;
  }
}

async function handleFile(file) {
  if (!file) {
    console.warn("[WOL Highlights Popup] handleFile called with no file.");
    return;
  }
  const startTime = performance.now();
  console.log("[WOL Highlights Popup] handleFile() started:", {
    name: file.name,
    size: file.size,
    type: file.type,
    lastModified: new Date(file.lastModified).toISOString(),
  });

  if (file.name.endsWith(".json") || file.type === "application/json") {
    setMessage(`Reading ${file.name}…`);
    try {
      const text = await file.text();
      const parsed = JSON.parse(text);
      if (!parsed || (!parsed.docs && !parsed.bible && !parsed.notes)) {
        throw new Error("Invalid highlights JSON file format.");
      }
      const previous = await currentIndex();
      const notes = parsed.notes || { docs: {}, bible: {} };
      const inputFields = parsed.inputFields || { docs: {}, bible: {} };
      let noteCount = parsed.noteCount || 0;
      if (!noteCount) {
        if (notes) {
          for (const doc of Object.values(notes.docs || {})) {
            for (const list of Object.values(doc)) {
              noteCount += Array.isArray(list) ? list.length : 1;
            }
          }
          for (const ch of Object.values(notes.bible || {})) {
            for (const list of Object.values(ch)) {
              noteCount += Array.isArray(list) ? list.length : 1;
            }
          }
        }
        if (inputFields) {
          for (const doc of Object.values(inputFields.docs || {})) {
            for (const val of Object.values(doc)) {
              if (val && String(val).trim()) noteCount++;
            }
          }
          for (const ch of Object.values(inputFields.bible || {})) {
            for (const val of Object.values(ch)) {
              if (val && String(val).trim()) noteCount++;
            }
          }
        }
      }
      const index = {
        version: 1,
        fileName: file.name,
        loadedAt: new Date().toISOString(),
        enabled: previous ? previous.enabled !== false : true,
        highlightCount: parsed.highlightCount || 0,
        noteCount,
        documentCount: parsed.documentCount || Object.keys(parsed.docs || {}).length,
        bibleChapterCount: parsed.bibleChapterCount || Object.keys(parsed.bible || {}).length,
        colorCounts: parsed.colorCounts || {},
        docs: parsed.docs || {},
        bible: parsed.bible || {},
        notes,
        inputFields,
      };
      await ext.storage.local.set({ wolMarks: index });
      renderIndex(index);
      const summaryParts = [];
      if (index.highlightCount) summaryParts.push(`${index.highlightCount.toLocaleString()} highlights`);
      if (index.noteCount) summaryParts.push(`${index.noteCount.toLocaleString()} notes`);
      setMessage(summaryParts.length ? `Loaded ${summaryParts.join(" and ")} from ${file.name}.` : "Data loaded.");
      await refreshPage();
      return;
    } catch (err) {
      console.error("[WOL Highlights Popup] Failed to parse JSON highlights file:", err);
      setMessage(err.message || "Could not read that JSON file.", true);
      return;
    }
  }

  setMessage(`Reading ${file.name} (${(file.size / 1024).toFixed(1)} KB)…`);
  try {
    const previous = await currentIndex();
    console.log("[WOL Highlights Popup] Previous stored backup:", previous ? { fileName: previous.fileName, highlights: previous.highlightCount } : "None");
    console.log("[WOL Highlights Popup] Reading array buffer...");
    const bufferStart = performance.now();
    const bytes = new Uint8Array(await file.arrayBuffer());
    console.log(`[WOL Highlights Popup] Read buffer (${bytes.length} bytes in ${(performance.now() - bufferStart).toFixed(1)}ms). Calling parseBackup...`);

    const wasmUrl = ext.runtime.getURL("lib/sql-wasm.wasm");
    console.log("[WOL Highlights Popup] Using WebAssembly URL:", wasmUrl);
    console.log("[WOL Highlights Popup] typeof globalThis.initSqlJs:", typeof globalThis.initSqlJs);

    if (typeof globalThis.initSqlJs !== "function") {
      throw new Error("sql.js library (globalThis.initSqlJs) is not loaded or not a function.");
    }

    setMessage(`Parsing ${file.name} with SQLite WebAssembly…`);
    const parseStart = performance.now();
    const index = await parseBackup(bytes, globalThis.initSqlJs, wasmUrl);
    console.log(`[WOL Highlights Popup] Backup successfully parsed in ${(performance.now() - parseStart).toFixed(1)}ms! Total highlights:`, index.highlightCount);

    index.fileName = file.name;
    index.loadedAt = new Date().toISOString();
    index.enabled = previous ? previous.enabled !== false : true;

    console.log("[WOL Highlights Popup] Saving index to storage.local (estimated size ~" + Math.round(JSON.stringify(index).length / 1024) + " KB)...");
    const saveStart = performance.now();
    await ext.storage.local.set({ wolMarks: index });
    console.log(`[WOL Highlights Popup] Stored wolMarks into storage.local in ${(performance.now() - saveStart).toFixed(1)}ms.`);

    renderIndex(index);
    const summaryParts = [];
    if (index.highlightCount) summaryParts.push(`${index.highlightCount.toLocaleString()} highlight${index.highlightCount === 1 ? "" : "s"}`);
    if (index.noteCount) summaryParts.push(`${index.noteCount.toLocaleString()} note${index.noteCount === 1 ? "" : "s"}`);
    setMessage(summaryParts.length ? `Loaded ${summaryParts.join(" and ")} from ${file.name}.` : "That backup has no highlights or notes.");
    console.log("[WOL Highlights Popup] Refreshing page highlights...");
    await refreshPage();
    console.log(`[WOL Highlights Popup] Entire file processing completed in ${(performance.now() - startTime).toFixed(1)}ms.`);
  } catch (error) {
    console.error("[WOL Highlights Popup] Error reading/parsing backup file:", error);
    setMessage(error.message || "Could not read that file.", true);
  }
}

// Lifecycle and focus tracking (helps debug if popup closes on file chooser)
window.addEventListener("focus", () => {
  console.log("[WOL Highlights Popup] Event: window 'focus'");
});
window.addEventListener("blur", () => {
  console.warn("[WOL Highlights Popup] Event: window 'blur'. (NOTE: If an OS file dialog opened, Firefox may destroy this popup!)");
});
window.addEventListener("pagehide", (event) => {
  console.warn("[WOL Highlights Popup] Event: window 'pagehide' (persisted=" + event.persisted + ")");
});
window.addEventListener("beforeunload", () => {
  console.warn("[WOL Highlights Popup] Event: window 'beforeunload'");
});

// File picker event monitoring
fileInput.addEventListener("click", () => {
  console.log("[WOL Highlights Popup] 'Load backup' button clicked. Native file picker dialog requested...");
});

fileInput.addEventListener("cancel", () => {
  console.log("[WOL Highlights Popup] Native file picker was cancelled by user.");
});

fileInput.addEventListener("input", () => {
  console.log("[WOL Highlights Popup] fileInput 'input' event fired. File count:", fileInput.files?.length);
});

fileInput.addEventListener("change", async () => {
  console.log("[WOL Highlights Popup] fileInput 'change' event fired. File count:", fileInput.files?.length);
  const file = fileInput.files?.[0];
  fileInput.value = "";
  if (!file) {
    console.log("[WOL Highlights Popup] File input changed with no file selected.");
    return;
  }
  console.log("[WOL Highlights Popup] File selected:", file.name, "(" + file.size + " bytes)");
  await handleFile(file);
});

// Drag & drop support
window.addEventListener("dragover", (event) => {
  event.preventDefault();
  document.body.classList.add("dragover");
});

window.addEventListener("dragleave", (event) => {
  if (event.relatedTarget === null) {
    document.body.classList.remove("dragover");
  }
});

window.addEventListener("drop", async (event) => {
  event.preventDefault();
  document.body.classList.remove("dragover");
  const file = event.dataTransfer?.files?.[0];
  console.log("[WOL Highlights Popup] Drop event received. File:", file ? `${file.name} (${file.size} bytes)` : "None");
  if (file) {
    await handleFile(file);
  }
});


enabledInput.addEventListener("change", async () => {
  const index = await currentIndex();
  if (!index) return;
  index.enabled = enabledInput.checked;
  console.log("[WOL Highlights Popup] Toggled highlights enabled:", index.enabled);
  await ext.storage.local.set({ wolMarks: index });
  await refreshPage();
});

removeButton.addEventListener("click", async () => {
  const index = await currentIndex();
  if (!index) return;
  const confirmed = window.confirm(`Remove the highlights loaded from ${index.fileName}?`);
  if (!confirmed) return;
  console.log("[WOL Highlights Popup] Removing backup from storage:", index.fileName);
  await ext.storage.local.remove("wolMarks");
  renderIndex(null);
  setMessage("Backup removed from the browser.");
  await refreshPage();
});

if (exportButton) {
  exportButton.addEventListener("click", async () => {
    const index = await currentIndex();
    if (!index || (!index.highlightCount && !index.noteCount)) {
      setMessage("No highlights or notes to export.", true);
      return;
    }
    const jsonStr = JSON.stringify(index, null, 2);
    const blob = new Blob([jsonStr], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `wol-study-backup-${new Date().toISOString().slice(0, 10)}.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    setMessage("Backup exported as JSON.");
  });
}

console.log("[WOL Highlights Popup] Initializing popup interface...");
const initial = await currentIndex();
console.log("[WOL Highlights Popup] Initial stored index:", initial ? { fileName: initial.fileName, highlights: initial.highlightCount } : "None");
renderIndex(initial);
await refreshPage();
await initCloudSync();

async function sendSyncMessage(type, payload) {
  const resp = await ext.runtime.sendMessage({ type, payload });
  if (!resp || !resp.ok) {
    throw new Error(resp?.error || "Sync request failed.");
  }
  return resp.data;
}

async function requestOriginPermission(serverUrl) {
  if (!ext.permissions?.request || !globalThis.WOLSync) return;
  try {
    const normalized = globalThis.WOLSync.normalizeNextcloudServerUrl(serverUrl);
    const origin = new URL(normalized).origin;
    await ext.permissions.request({ origins: [`${origin}/*`] });
  } catch (err) {
    console.warn("[WOL Highlights Popup] Optional host permission request:", err);
  }
}

async function initCloudSync() {
  const disconnectedEl = document.querySelector("#sync-disconnected");
  const connectedEl = document.querySelector("#sync-connected");
  const btnGoogle = document.querySelector("#sync-google");
  const btnNcToggle = document.querySelector("#sync-nc-toggle");
  const ncForm = document.querySelector("#sync-nc-form");
  const ncServer = document.querySelector("#nc-server");
  const ncBrowser = document.querySelector("#nc-browser");
  const ncUsername = document.querySelector("#nc-username");
  const ncPassword = document.querySelector("#nc-password");
  const ncRemotePath = document.querySelector("#nc-remote-path");
  const ncManual = document.querySelector("#nc-manual");
  const syncBadge = document.querySelector("#sync-badge");
  const syncAccount = document.querySelector("#sync-account");
  const syncNowBtn = document.querySelector("#sync-now");
  const disconnectBtn = document.querySelector("#sync-disconnect");
  const syncMeta = document.querySelector("#sync-meta");
  const syncErr = document.querySelector("#sync-err");

  function renderSyncStatus(status) {
    if (!status || !status.connected) {
      disconnectedEl.hidden = false;
      connectedEl.hidden = true;
      return;
    }
    disconnectedEl.hidden = true;
    connectedEl.hidden = false;
    syncBadge.textContent = status.provider === "googledrive" ? "Google Drive" : "Nextcloud";
    syncAccount.textContent = status.serverUrl
      ? `${status.accountLabel || ""} @ ${status.serverUrl}`
      : (status.accountLabel || "Connected");
    const last = status.lastSyncedAt ? formatWhen(status.lastSyncedAt) : "Never";
    syncMeta.textContent = status.syncing ? "Syncing…" : `Last synced: ${last}`;
    if (status.lastError) {
      syncErr.textContent = `Last sync error: ${status.lastError}`;
      syncErr.hidden = false;
    } else {
      syncErr.hidden = true;
    }
  }

  async function refreshSyncStatus() {
    try {
      const status = await sendSyncMessage("wol-sync-status");
      renderSyncStatus(status);
    } catch (err) {
      console.warn("[WOL Highlights Popup] sync status error:", err);
    }
  }

  ext.storage.onChanged.addListener(async (changes, area) => {
    if (area !== "local") return;
    if (changes.wolMarks) {
      renderIndex(await currentIndex());
    }
    if (changes.wolSync) {
      await refreshSyncStatus();
    }
  });

  btnNcToggle?.addEventListener("click", () => {
    ncForm.hidden = !ncForm.hidden;
  });

  btnGoogle?.addEventListener("click", async () => {
    btnGoogle.disabled = true;
    setMessage("Opening Google sign-in…");
    try {
      const status = await sendSyncMessage("wol-sync-google");
      renderSyncStatus(status);
      renderIndex(await currentIndex());
      await refreshPage();
      setMessage("Google Drive connected and synced!");
    } catch (err) {
      setMessage(err.message || "Google Drive sign-in failed.", true);
    } finally {
      btnGoogle.disabled = false;
    }
  });

  ncBrowser?.addEventListener("click", async () => {
    const serverUrl = ncServer.value.trim();
    if (!serverUrl) {
      setMessage("Please enter your Nextcloud server URL.", true);
      return;
    }
    ncBrowser.disabled = true;
    setMessage("Opening Nextcloud sign-in…");
    try {
      await requestOriginPermission(serverUrl);
      const status = await sendSyncMessage("wol-sync-nextcloud-browser", {
        serverUrl,
        remotePath: ncRemotePath.value.trim() || null,
      });
      renderSyncStatus(status);
      renderIndex(await currentIndex());
      await refreshPage();
      setMessage("Nextcloud connected and synced!");
    } catch (err) {
      setMessage(err.message || "Nextcloud sign-in failed.", true);
    } finally {
      ncBrowser.disabled = false;
    }
  });

  ncManual?.addEventListener("click", async () => {
    const serverUrl = ncServer.value.trim();
    const username = ncUsername.value.trim();
    const password = ncPassword.value;
    if (!serverUrl || !username || !password) {
      setMessage("Please fill in Server URL, Username, and App Password.", true);
      return;
    }
    ncManual.disabled = true;
    setMessage("Connecting to Nextcloud…");
    try {
      await requestOriginPermission(serverUrl);
      const status = await sendSyncMessage("wol-sync-nextcloud", {
        serverUrl,
        username,
        password,
        remotePath: ncRemotePath.value.trim() || null,
      });
      renderSyncStatus(status);
      renderIndex(await currentIndex());
      await refreshPage();
      setMessage("Nextcloud connected and synced!");
    } catch (err) {
      setMessage(err.message || "Nextcloud connection failed.", true);
    } finally {
      ncManual.disabled = false;
    }
  });

  syncNowBtn?.addEventListener("click", async () => {
    syncNowBtn.disabled = true;
    syncMeta.textContent = "Syncing…";
    try {
      const status = await sendSyncMessage("wol-sync-now");
      renderSyncStatus(status);
      if (status.changedLocal) {
        renderIndex(await currentIndex());
        await refreshPage();
      }
      setMessage("Sync complete.");
    } catch (err) {
      setMessage(err.message || "Sync failed.", true);
      await refreshSyncStatus();
    } finally {
      syncNowBtn.disabled = false;
    }
  });

  disconnectBtn?.addEventListener("click", async () => {
    if (!window.confirm("Disconnect cloud sync? Your local highlights will stay in the browser.")) {
      return;
    }
    try {
      const status = await sendSyncMessage("wol-sync-disconnect");
      renderSyncStatus(status);
      setMessage("Cloud sync disconnected.");
    } catch (err) {
      setMessage(err.message || "Disconnect failed.", true);
    }
  });

  await refreshSyncStatus();
}
