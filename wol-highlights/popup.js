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
  counts.textContent = `${index.highlightCount.toLocaleString()} highlight${index.highlightCount === 1 ? "" : "s"}`
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
  if (!status.enabled && status.hasBackup) return "Highlights are hidden.";
  if (status.applied > 0) {
    const bits = [`${status.applied} highlight${status.applied === 1 ? "" : "s"} shown`];
    if (status.missing) bits.push(`${status.missing} not on this page`);
    if (status.skipped) bits.push(`${status.skipped} did not match the page text`);
    return bits.join(". ") + ".";
  }
  return "Select any text on this page to highlight it with JW Library colors.";
}

async function refreshPage() {
  console.log("[WOL Highlights Popup] refreshPage() querying active tab...");
  let tabs = [];
  try {
    tabs = await ext.tabs.query({ active: true, currentWindow: true });
  } catch (err) {
    console.error("[WOL Highlights Popup] Failed to query active tabs:", err);
  }
  const tab = tabs[0];
  console.log("[WOL Highlights Popup] Active tab:", tab ? { id: tab.id, url: tab.url, title: tab.title } : "No active tab");
  if (!tab?.id || !tab.url || !tab.url.startsWith("https://wol.jw.org/")) {
    pageStatus.textContent = "Open an article on wol.jw.org.";
    console.log("[WOL Highlights Popup] Tab is not on wol.jw.org.");
    return;
  }
  try {
    console.log("[WOL Highlights Popup] Sending 'wol-reapply' message to tab ID " + tab.id + " (" + tab.url + ")...");
    const status = await ext.tabs.sendMessage(tab.id, { type: "wol-reapply" });
    console.log("[WOL Highlights Popup] Received status from content script:", status);
    pageStatus.textContent = describePage(status);
  } catch (err) {
    console.warn("[WOL Highlights Popup] Could not communicate with content script on tab " + tab.id + " (reload the article tab if it was opened before the extension was loaded):", err);
    pageStatus.textContent = "Reload the article tab, then open this popup again.";
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
      if (!parsed || (!parsed.docs && !parsed.bible)) {
        throw new Error("Invalid highlights JSON file format.");
      }
      const previous = await currentIndex();
      const index = {
        version: 1,
        fileName: file.name,
        loadedAt: new Date().toISOString(),
        enabled: previous ? previous.enabled !== false : true,
        highlightCount: parsed.highlightCount || 0,
        documentCount: parsed.documentCount || Object.keys(parsed.docs || {}).length,
        bibleChapterCount: parsed.bibleChapterCount || Object.keys(parsed.bible || {}).length,
        colorCounts: parsed.colorCounts || {},
        docs: parsed.docs || {},
        bible: parsed.bible || {},
      };
      await ext.storage.local.set({ wolMarks: index });
      renderIndex(index);
      setMessage(index.highlightCount ? `Loaded ${index.highlightCount.toLocaleString()} highlights from ${file.name}.` : "Highlights loaded.");
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
    setMessage(index.highlightCount ? `Loaded ${index.highlightCount.toLocaleString()} highlights from ${file.name}.` : "That backup has no highlights.");
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

function openInPermanentTab(e) {
  if (e) e.preventDefault();
  console.log("[WOL Highlights Popup] Opening popup in permanent browser tab...");
  ext.tabs.create({ url: ext.runtime.getURL("popup.html") });
}

const openTabLink = document.querySelector("#open-tab");
if (openTabLink) {
  openTabLink.addEventListener("click", openInPermanentTab);
}

const openTabBtn = document.querySelector("#open-tab-btn");
if (openTabBtn) {
  openTabBtn.addEventListener("click", openInPermanentTab);
}

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
    if (!index || !index.highlightCount) {
      setMessage("No highlights to export.", true);
      return;
    }
    const jsonStr = JSON.stringify(index, null, 2);
    const blob = new Blob([jsonStr], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `wol-highlights-${new Date().toISOString().slice(0, 10)}.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    setMessage("Highlights exported as JSON.");
  });
}

console.log("[WOL Highlights Popup] Initializing popup interface...");
const initial = await currentIndex();
console.log("[WOL Highlights Popup] Initial stored index:", initial ? { fileName: initial.fileName, highlights: initial.highlightCount } : "None");
renderIndex(initial);
await refreshPage();
