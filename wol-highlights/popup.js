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
  const stored = await ext.storage.local.get("wolMarks");
  return stored.wolMarks && stored.wolMarks.version === 1 ? stored.wolMarks : null;
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
  if (!status.hasBackup) return "Load a backup to mark this page.";
  if (!status.enabled) return "Highlights are hidden.";
  if (!status.docId && !status.bibleLabel && status.applied === 0) {
    return "This page is not an article or Bible chapter in the backup.";
  }
  const bits = [];
  if (status.applied) bits.push(`${status.applied} highlight${status.applied === 1 ? "" : "s"} shown`);
  else bits.push("No highlights shown");
  if (status.missing) bits.push(`${status.missing} not on this page`);
  if (status.skipped) bits.push(`${status.skipped} did not match the page text`);
  return bits.join(". ") + ".";
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
  if (!file) return;
  console.log("[WOL Highlights Popup] handleFile() received:", {
    name: file.name,
    size: file.size,
    type: file.type,
    lastModified: file.lastModified,
  });
  setMessage(`Reading ${file.name}…`);
  try {
    const previous = await currentIndex();
    console.log("[WOL Highlights Popup] Previous stored backup:", previous ? { fileName: previous.fileName, highlights: previous.highlightCount } : "None");
    console.log("[WOL Highlights Popup] Reading array buffer...");
    const bytes = new Uint8Array(await file.arrayBuffer());
    console.log("[WOL Highlights Popup] Read buffer (" + bytes.length + " bytes). Calling parseBackup...");

    const wasmUrl = ext.runtime.getURL("lib/sql-wasm.wasm");
    console.log("[WOL Highlights Popup] Using WebAssembly URL:", wasmUrl);
    console.log("[WOL Highlights Popup] initSqlJs available:", typeof globalThis.initSqlJs);

    const index = await parseBackup(bytes, globalThis.initSqlJs, wasmUrl);
    console.log("[WOL Highlights Popup] Backup successfully parsed! Total highlights:", index.highlightCount);

    index.fileName = file.name;
    index.loadedAt = new Date().toISOString();
    index.enabled = previous ? previous.enabled !== false : true;

    console.log("[WOL Highlights Popup] Saving index to storage.local...");
    await ext.storage.local.set({ wolMarks: index });
    console.log("[WOL Highlights Popup] Stored wolMarks into storage.local.");

    renderIndex(index);
    setMessage(index.highlightCount ? "" : "That backup has no highlights.");
    console.log("[WOL Highlights Popup] Refreshing page highlights...");
    await refreshPage();
    console.log("[WOL Highlights Popup] File processing complete.");
  } catch (error) {
    console.error("[WOL Highlights Popup] Error reading/parsing backup file:", error);
    setMessage(error.message || "Could not read that file.", true);
  }
}

fileInput.addEventListener("change", async () => {
  const file = fileInput.files?.[0];
  fileInput.value = "";
  if (!file) {
    console.log("[WOL Highlights Popup] File input changed with no file selected.");
    return;
  }
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
  if (file) {
    console.log("[WOL Highlights Popup] File dropped into popup:", file.name);
    await handleFile(file);
  }
});

const openTabLink = document.querySelector("#open-tab");
if (openTabLink) {
  openTabLink.addEventListener("click", (e) => {
    e.preventDefault();
    console.log("[WOL Highlights Popup] Opening popup in full tab...");
    ext.tabs.create({ url: ext.runtime.getURL("popup.html") });
  });
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

console.log("[WOL Highlights Popup] Initializing popup interface...");
const initial = await currentIndex();
console.log("[WOL Highlights Popup] Initial stored index:", initial ? { fileName: initial.fileName, highlights: initial.highlightCount } : "None");
renderIndex(initial);
await refreshPage();
