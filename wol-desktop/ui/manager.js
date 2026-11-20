// Bridge setup for Tauri environment
const tauriInvoke = window.__TAURI_INTERNALS__?.invoke;
import { importJwpub } from "./jwpub-import.mjs";

document.querySelector("#import-jwpub")?.addEventListener("change", async (event) => {
  const file = event.target.files?.[0];
  if (!file) return;
  const status = document.querySelector("#import-jwpub-status");
  status.textContent = "Importing publication…";
  try {
    const book = await importJwpub(new Uint8Array(await file.arrayBuffer()), window.initSqlJs);
    await tauriInvoke("save_imported_publication", { publication: book });
    status.textContent = `Imported ${book.title} (${book.chapters.length} articles). Open Publications in the main window.`;
  } catch (error) {
    status.textContent = `Could not import publication: ${error.message}`;
  } finally { event.target.value = ""; }
});

export const bridge = {
  storage: {
    local: {
      get: async function(keys) {
        let currentMarks = null;
        try {
          if (tauriInvoke) {
            currentMarks = await tauriInvoke("get_marks");
          }
        } catch (e) {
          console.warn("[Bridge] invoke get_marks failed:", e);
        }
        if (keys === "wolMarks" || keys === null || keys === undefined) {
          return { wolMarks: currentMarks };
        }
        if (Array.isArray(keys)) {
          const out = {};
          if (keys.includes("wolMarks")) out.wolMarks = currentMarks;
          return out;
        }
        return { wolMarks: currentMarks };
      },
      set: async function(items) {
        if (items && items.wolMarks !== undefined) {
          try {
            if (tauriInvoke) {
              await tauriInvoke("set_marks", { marks: items.wolMarks });
            }
          } catch (e) {
            console.warn("[Bridge] invoke set_marks failed:", e);
          }
        }
      },
      remove: async function(key) {
        if (key === "wolMarks") {
          try {
            if (tauriInvoke) {
              await tauriInvoke("set_marks", { marks: null });
            }
          } catch (e) {
            console.warn("[Bridge] invoke set_marks failed:", e);
          }
        }
      }
    }
  },
  runtime: {
    getURL: function(path) {
      return path;
    },
    onMessage: {
      addListener: function() {}
    }
  },
  tabs: {
    query: async function() {
      return [{ id: "main", url: "wol://main" }];
    },
    sendMessage: async function(_tabId, message) {
      if (tauriInvoke) {
        return await tauriInvoke("dispatch_tab_message", { message });
      }
      return { applied: 1, hasBackup: true, enabled: true };
    }
  }
};

window.chrome = bridge;
window.browser = bridge;
const ext = bridge;

// Constants
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

// DOM Elements - Highlights
const fileInput = document.querySelector("#file");
const loadedSection = document.querySelector("#loaded");
const fileName = document.querySelector("#file-name");
const counts = document.querySelector("#counts");
const colorBar = document.querySelector("#color-bar");
const colors = document.querySelector("#colors");
const enabledInput = document.querySelector("#enabled");
const removeButton = document.querySelector("#remove");
const exportButton = document.querySelector("#export-json");
const resetAllButton = document.querySelector("#reset-all");
const pageStatus = document.querySelector("#page-status");
const message = document.querySelector("#message");

// DOM Elements - Settings
const settingLang = document.querySelector("#setting-lang");
const settingHome = document.querySelector("#setting-home");
const settingTheme = document.querySelector("#setting-theme");
const settingCap = document.querySelector("#setting-cap");

// DOM Elements - Cache Inspector
const statTotalSize = document.querySelector("#stat-total-size");
const statEntryCount = document.querySelector("#stat-entry-count");
const statCapSize = document.querySelector("#stat-cap-size");
const btnCrawlCurrent = document.querySelector("#btn-crawl-current");
const btnClearOlder = document.querySelector("#btn-clear-older");
const btnClearAll = document.querySelector("#btn-clear-all");
const cacheSearch = document.querySelector("#cache-search");
const cacheEntriesBody = document.querySelector("#cache-entries-body");

// DOM Elements - Network Status
const networkDot = document.querySelector("#network-dot");
const networkText = document.querySelector("#network-text");

let languagesList = [];
let currentSettings = null;

// ----------------------------------------------------
// THEME
// ----------------------------------------------------
const managerDarkQuery = window.matchMedia ? window.matchMedia("(prefers-color-scheme: dark)") : null;
let currentThemeMode = "system";

function applyManagerTheme(mode) {
  currentThemeMode = mode || "system";
  const isDark =
    currentThemeMode === "dark" ||
    (currentThemeMode === "system" && Boolean(managerDarkQuery && managerDarkQuery.matches));
  document.documentElement.setAttribute("data-theme", isDark ? "dark" : "light");
}

window.__wolApplyManagerTheme = applyManagerTheme;
if (managerDarkQuery && typeof managerDarkQuery.addEventListener === "function") {
  managerDarkQuery.addEventListener("change", () => {
    if (currentThemeMode === "system") {
      applyManagerTheme("system");
    }
  });
}
applyManagerTheme("system");

// Helper: Format bytes
function formatBytes(bytes) {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

function setMessage(text, isError = false) {
  if (!message) return;
  message.textContent = text || "";
  message.classList.toggle("error", Boolean(isError));
}

function formatWhen(iso) {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleString();
}

// ----------------------------------------------------
// TAB SWITCHING
// ----------------------------------------------------
function selectTab(tabId) {
  const targetBtn = document.querySelector(`.tab-btn[data-tab="${tabId}"]`);
  const targetPane = document.getElementById(tabId);
  if (!targetBtn || !targetPane) return;

  document.querySelectorAll(".tab-btn").forEach((b) => {
    b.classList.remove("active");
    b.setAttribute("aria-selected", "false");
  });
  document.querySelectorAll(".tab-pane").forEach((p) => p.classList.remove("active"));
  targetBtn.classList.add("active");
  targetBtn.setAttribute("aria-selected", "true");
  targetPane.classList.add("active");

  if (tabId === "tab-cache") {
    refreshCacheInspector();
  }
}

window.__wolSelectTab = selectTab;

document.querySelectorAll(".tab-btn").forEach((btn) => {
  btn.addEventListener("click", () => selectTab(btn.dataset.tab));
});

if (window.location.hash && window.location.hash.length > 1) {
  selectTab(window.location.hash.slice(1));
}

// ----------------------------------------------------
// MOBILE SINGLE-WINDOW BACK NAVIGATION
// ----------------------------------------------------
let languageChanged = false;
const mobileBackBtn = document.querySelector("#mobile-back-btn");
if (mobileBackBtn) {
  const winLabel = window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label;
  if (winLabel === "main") {
    mobileBackBtn.hidden = false;
  }
  mobileBackBtn.addEventListener("click", async () => {
    if (!languageChanged && window.history.length > 1) {
      window.history.back();
      return;
    }
    if (tauriInvoke) {
      try {
        await tauriInvoke("navigate_home");
      } catch (err) {
        console.warn("[Bridge] navigate_home failed:", err);
      }
    }
  });
}

// ----------------------------------------------------
// NETWORK STATUS
// ----------------------------------------------------
function updateNetworkStatus() {
  const online = navigator.onLine;
  if (networkDot) networkDot.classList.toggle("offline", !online);
  if (networkText) networkText.textContent = online ? "Online" : "Offline";
}
window.addEventListener("online", updateNetworkStatus);
window.addEventListener("offline", updateNetworkStatus);
updateNetworkStatus();

// ----------------------------------------------------
// HIGHLIGHTS MANAGER
// ----------------------------------------------------
async function currentIndex() {
  try {
    const all = await ext.storage.local.get(null);
    const stored = all.wolMarks;
    if (!stored || stored.version !== 1) return null;
    return stored;
  } catch (err) {
    console.error("[Manager] Failed to read from ext.storage.local:", err);
    return null;
  }
}

function renderIndex(index) {
  const hasExportable = Boolean(index && (index.highlightCount || index.noteCount));
  if (exportButton) {
    exportButton.disabled = !hasExportable;
    exportButton.title = hasExportable
      ? "Choose where to save a .jwlibrary backup file"
      : "Load or create highlights first to export";
  }
  if (resetAllButton) {
    resetAllButton.disabled = !hasExportable;
  }

  if (!index) {
    loadedSection.hidden = true;
    return;
  }
  loadedSection.hidden = false;
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
  const entries = Object.entries(index.colorCounts || {});
  const totalColors = entries.reduce((sum, [, c]) => sum + c, 0);
  if (colorBar) {
    colorBar.replaceChildren();
    colorBar.hidden = totalColors === 0;
    for (const [color, count] of entries) {
      if (!count) continue;
      const seg = document.createElement("span");
      seg.className = "color-bar-seg";
      seg.style.flexGrow = String(count);
      seg.style.background = COLOR_CSS[color] || "#e6e6e6";
      seg.title = `${COLOR_NAMES[color] || "Other"}: ${count}`;
      colorBar.append(seg);
    }
  }
  for (const [color, count] of entries) {
    const item = document.createElement("li");
    const swatch = document.createElement("span");
    swatch.className = "swatch";
    swatch.style.background = COLOR_CSS[color] || "#e6e6e6";
    item.append(swatch, `${COLOR_NAMES[color] || "Other"} ${count}`);
    colors.append(item);
  }
  enabledInput.checked = index.enabled !== false;
}

async function refreshPage() {
  try {
    const tabs = await ext.tabs.query();
    if (!tabs || !tabs.length) {
      if (pageStatus) pageStatus.textContent = "Open an article in the main window.";
      return;
    }
    const res = await ext.tabs.sendMessage("main", { type: "wol-reapply" });
    if (pageStatus) {
      if (res && res.applied !== undefined) {
        pageStatus.textContent = `Highlights active. Updates applied.`;
      } else {
        pageStatus.textContent = `Main window updated.`;
      }
    }
  } catch (err) {
    console.warn("[Manager] refreshPage error:", err);
  }
}

async function handleFile(file) {
  if (!file) return;
  const startTime = performance.now();

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
      setMessage(`Loaded highlights and notes from ${file.name}.`);
      await refreshPage();
      return;
    } catch (err) {
      console.error("[Manager] Failed to parse JSON file:", err);
      setMessage(err.message || "Could not read that JSON file.", true);
      return;
    }
  }

  // Handle .jwlibrary, .db, .sqlite
  setMessage(`Reading ${file.name} (${(file.size / 1024).toFixed(1)} KB)…`);
  try {
    const previous = await currentIndex();
    const bytes = new Uint8Array(await file.arrayBuffer());
    const wasmUrl = ext.runtime.getURL("lib/sql-wasm.wasm");

    if (typeof globalThis.initSqlJs !== "function") {
      throw new Error("sql.js library (globalThis.initSqlJs) is not loaded.");
    }

    setMessage(`Parsing ${file.name} with SQLite WebAssembly…`);
    const index = await parseBackup(bytes, globalThis.initSqlJs, wasmUrl);

    index.fileName = file.name;
    index.loadedAt = new Date().toISOString();
    index.enabled = previous ? previous.enabled !== false : true;

    await ext.storage.local.set({ wolMarks: index });
    renderIndex(index);
    setMessage(`Loaded ${index.highlightCount} highlights from ${file.name}.`);
    await refreshPage();
    console.log(`[Manager] Processed ${file.name} in ${(performance.now() - startTime).toFixed(1)}ms.`);
  } catch (error) {
    console.error("[Manager] Error parsing backup:", error);
    setMessage(error.message || "Could not read that file.", true);
  }
}

// File chooser & drag-and-drop
fileInput.addEventListener("change", async () => {
  const file = fileInput.files?.[0];
  fileInput.value = "";
  if (file) await handleFile(file);
});

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
  if (file) await handleFile(file);
});

enabledInput.addEventListener("change", async () => {
  const index = await currentIndex();
  if (!index) return;
  index.enabled = enabledInput.checked;
  await ext.storage.local.set({ wolMarks: index });
  await refreshPage();
});

removeButton.addEventListener("click", async () => {
  const index = await currentIndex();
  if (!index) return;
  if (!window.confirm(`Remove highlights loaded from ${index.fileName}?`)) return;
  await ext.storage.local.remove("wolMarks");
  renderIndex(null);
  setMessage("Backup removed.");
  await refreshPage();
});

if (resetAllButton) {
  resetAllButton.addEventListener("click", async () => {
    const index = await currentIndex();
    if (!index) return;
    if (!window.confirm("Reset all highlights and comments? This will clear all markers and notes so you can start fresh.")) return;
    await ext.storage.local.remove("wolMarks");
    renderIndex(null);
    setMessage("All highlights and comments have been reset.");
    await refreshPage();
  });
}

if (exportButton) {
  exportButton.addEventListener("click", async () => {
    const index = await currentIndex();
    if (!index || (!index.highlightCount && !index.noteCount)) {
      setMessage("No highlights or notes to export.", true);
      return;
    }
    const dateStr = new Date().toISOString().slice(0, 10);
    if (tauriInvoke) {
      const defaultName = `UserdataBackup_${dateStr}_OfflineWOL.jwlibrary`;
      try {
        const savedPath = await tauriInvoke("export_marks_to_file", { defaultName });
        if (savedPath) {
          setMessage(`JW Library backup exported to ${savedPath}.`);
        } else {
          setMessage("Export cancelled.");
        }
        return;
      } catch (err) {
        const msg = typeof err === "string" ? err : (err?.message || "");
        if (!msg.includes("export_marks_to_bytes") && !msg.includes("not supported on mobile")) {
          console.error("[Manager] Export failed:", err);
          setMessage(msg || "Export failed.", true);
          return;
        }
      }

      try {
        const rawBytes = await tauriInvoke("export_marks_to_bytes", { defaultName });
        const bytes = new Uint8Array(rawBytes);
        const file = new File([bytes], defaultName, { type: "application/zip" });
        if (typeof navigator.share === "function" && navigator.canShare?.({ files: [file] })) {
          await navigator.share({ files: [file], title: defaultName });
          setMessage(`JW Library backup ready to share (${defaultName}).`);
        } else {
          const url = URL.createObjectURL(file);
          const a = document.createElement("a");
          a.href = url;
          a.download = defaultName;
          document.body.appendChild(a);
          a.click();
          document.body.removeChild(a);
          URL.revokeObjectURL(url);
          setMessage(`JW Library backup exported (${defaultName}).`);
        }
      } catch (err) {
        if (err?.name === "AbortError") {
          setMessage("Export cancelled.");
          return;
        }
        console.error("[Manager] Mobile export failed:", err);
        setMessage(typeof err === "string" ? err : (err?.message || "Export failed."), true);
      }
      return;
    }

    const defaultName = `wol-study-backup-${dateStr}.json`;

    const jsonStr = JSON.stringify(index, null, 2);
    const blob = new Blob([jsonStr], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = defaultName;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    setMessage("Backup exported as JSON.");
  });
}

// ----------------------------------------------------
// SETTINGS
// ----------------------------------------------------
async function initSettings() {
  if (!tauriInvoke) return;
  try {
    languagesList = await tauriInvoke("get_languages");
    currentSettings = await tauriInvoke("get_settings");

    let activeBaseHost = currentSettings.baseHost || "";
    if (!activeBaseHost && currentSettings.home) {
      try {
        activeBaseHost = new URL(currentSettings.home).host;
      } catch {}
    }

    const settingLangSearch = document.getElementById("setting-lang-search");
    const settingLangList = document.getElementById("setting-lang-list");
    const settingLangCurrent = document.getElementById("setting-lang-current");

    const resolveLangHome = (lang) =>
      activeBaseHost && lang.home.startsWith("/")
        ? `https://${activeBaseHost}${lang.home}`
        : lang.home;

    const updateCurrentLangBadge = (locale) => {
      if (!settingLangCurrent) return;
      const found = languagesList.find(
        (l) => l.locale === locale || l.symbol === locale
      );
      settingLangCurrent.textContent = found
        ? `${found.name} (${found.symbol})`
        : locale || "";
    };

    // Populate hidden language select (kept as backing value for persistSettings)
    settingLang.innerHTML = "";
    for (const lang of languagesList) {
      const opt = document.createElement("option");
      opt.value = lang.locale;
      opt.dataset.region = lang.region;
      opt.dataset.symbol = lang.symbol;
      opt.dataset.home = resolveLangHome(lang);
      opt.textContent = `${lang.name} (${lang.symbol}) - ${lang.locale}`;
      if (lang.locale === currentSettings.language || lang.symbol === currentSettings.language) {
        opt.selected = true;
      }
      settingLang.appendChild(opt);
    }
    updateCurrentLangBadge(settingLang.value);

    const renderLanguageList = (query) => {
      if (!settingLangList) return;
      const q = (query || "").trim().toLowerCase();
      const filtered = q
        ? languagesList.filter(
            (l) =>
              l.name.toLowerCase().includes(q) ||
              l.locale.toLowerCase().includes(q) ||
              l.symbol.toLowerCase().includes(q)
          )
        : languagesList;

      settingLangList.innerHTML = "";
      if (filtered.length === 0) {
        const empty = document.createElement("div");
        empty.className = "lang-picker-empty";
        empty.textContent = "No matching languages found.";
        settingLangList.appendChild(empty);
        return;
      }

      const activeLocale = settingLang.value;
      let selectedEl = null;
      for (const lang of filtered) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "lang-picker-item";
        const isSelected = lang.locale === activeLocale || lang.symbol === activeLocale;
        if (isSelected) {
          btn.classList.add("is-selected");
          selectedEl = btn;
        }
        btn.setAttribute("role", "option");
        btn.setAttribute("aria-selected", isSelected ? "true" : "false");

        const nameSpan = document.createElement("span");
        nameSpan.textContent = lang.name;

        const metaSpan = document.createElement("span");
        metaSpan.className = "lang-picker-meta";
        metaSpan.textContent = `${lang.symbol} · ${lang.locale}`;

        btn.appendChild(nameSpan);
        btn.appendChild(metaSpan);
        btn.addEventListener("click", async () => {
          settingLang.value = lang.locale;
          settingHome.value = resolveLangHome(lang);
          updateCurrentLangBadge(lang.locale);
          if (settingLangSearch) settingLangSearch.value = "";
          settingLangList.hidden = true;
          await persistSettings("Primary language updated. Click Back to Reader to open this library.");
          languageChanged = true;
        });
        settingLangList.appendChild(btn);
      }

      if (selectedEl && !q) {
        selectedEl.scrollIntoView({ block: "nearest" });
      }
    };

    if (settingLangSearch && settingLangList) {
      settingLangSearch.addEventListener("focus", () => {
        renderLanguageList(settingLangSearch.value);
        settingLangList.hidden = false;
      });
      settingLangSearch.addEventListener("input", () => {
        renderLanguageList(settingLangSearch.value);
        settingLangList.hidden = false;
      });
      settingLangSearch.addEventListener("keydown", (e) => {
        if (e.key === "Escape") {
          settingLangList.hidden = true;
          settingLangSearch.blur();
        }
      });
      document.addEventListener("click", (e) => {
        if (
          !settingLangList.hidden &&
          e.target !== settingLangSearch &&
          !settingLangList.contains(e.target)
        ) {
          settingLangList.hidden = true;
        }
      });
    }

    settingHome.value = currentSettings.home;
    const initialTheme = currentSettings.theme || "system";
    if (settingTheme) {
      settingTheme.value = initialTheme;
    }
    const syncThemeCards = (mode) => {
      document.querySelectorAll(".theme-card").forEach((card) => {
        card.classList.toggle("is-selected", card.dataset.themeValue === mode);
      });
    };

    const persistSettings = async (feedback) => {
      const selected = settingLang.selectedOptions[0];
      let baseHost = currentSettings?.baseHost || activeBaseHost || null;
      if (!baseHost && settingHome.value) {
        try {
          baseHost = new URL(settingHome.value).host || null;
        } catch {}
      }
      const updated = {
        baseHost,
        language: settingLang.value,
        region: parseInt(selected?.dataset.region || "4", 10),
        home: settingHome.value,
        cacheCapBytes: parseInt(settingCap.value, 10),
        theme: settingTheme ? settingTheme.value : "system",
      };
      await tauriInvoke("save_settings", { settings: updated });
      currentSettings = updated;
      if (feedback) setMessage(feedback);
    };

    syncThemeCards(initialTheme);
    document.querySelectorAll(".theme-card").forEach((card) => {
      card.addEventListener("click", async () => {
        const mode = card.dataset.themeValue || "system";
        if (settingTheme) settingTheme.value = mode;
        syncThemeCards(mode);
        applyManagerTheme(mode);
        await persistSettings();
      });
    });
    applyManagerTheme(initialTheme);
    settingCap.value = String(currentSettings.cacheCapBytes);

    settingCap.addEventListener("change", async () => {
      await persistSettings("Offline cache cap updated.");
      await refreshCacheInspector();
    });

    settingLang.addEventListener("change", async () => {
      const selected = settingLang.selectedOptions[0];
      if (selected?.dataset.home) {
        settingHome.value = selected.dataset.home;
      }
      await persistSettings("Primary language updated. Click Back to Reader to open this library.");
      languageChanged = true;
    });

    // WOL reading & behavior preferences (stored as WOL cookies in StateManager)
    const cookies = (await tauriInvoke("get_cookies")) || {};
    const settingFontScale = document.getElementById("setting-font-scale");
    const settingRuby = document.getElementById("setting-ruby");
    const settingAudioPlayer = document.getElementById("setting-audio-player");
    const settingAudioContinuous = document.getElementById("setting-audio-continuous");
    const settingSearchProx = document.getElementById("setting-search-prox");
    const settingSearchRank = document.getElementById("setting-search-rank");

    const updateWolCookie = async (name, value, message) => {
      await tauriInvoke("set_cookie", { name, value });
      if (message) setMessage(message);
    };

    if (settingFontScale) {
      const rawScale = parseFloat(cookies.fontScale || "1");
      let bestOption = "1";
      let bestDiff = Number.MAX_VALUE;
      for (const opt of settingFontScale.options) {
        const diff = Math.abs(rawScale - parseFloat(opt.value));
        if (diff < bestDiff) {
          bestDiff = diff;
          bestOption = opt.value;
        }
      }
      settingFontScale.value = bestOption;
      settingFontScale.addEventListener("change", () =>
        updateWolCookie("fontScale", settingFontScale.value, "Font size updated.")
      );
    }

    if (settingRuby) {
      settingRuby.checked = cookies.rb === "true";
      settingRuby.addEventListener("change", () =>
        updateWolCookie("rb", settingRuby.checked ? "true" : "false", "Ruby characters preference updated.")
      );
    }

    if (settingAudioPlayer) {
      // WOL defaults audioPlayer to visible when unset
      settingAudioPlayer.checked = cookies.audioPlayer !== "false";
      settingAudioPlayer.addEventListener("change", () =>
        updateWolCookie("audioPlayer", settingAudioPlayer.checked ? "true" : "false", "Audio player preference updated.")
      );
    }

    if (settingAudioContinuous) {
      // WOL defaults audioContinuousPlay to enabled when unset
      settingAudioContinuous.checked = cookies.audioContinuousPlay !== "false";
      settingAudioContinuous.addEventListener("change", () =>
        updateWolCookie("audioContinuousPlay", settingAudioContinuous.checked ? "true" : "false", "Audio auto-play preference updated.")
      );
    }

    if (settingSearchProx) {
      settingSearchProx.value = ["sen", "par", "doc"].includes(cookies.prox) ? cookies.prox : "par";
      settingSearchProx.addEventListener("change", () =>
        updateWolCookie("prox", settingSearchProx.value, "Default search scope updated.")
      );
    }

    if (settingSearchRank) {
      settingSearchRank.value = ["occ", "newest", "oldest"].includes(cookies.rank) ? cookies.rank : "occ";
      settingSearchRank.addEventListener("change", () =>
        updateWolCookie("rank", settingSearchRank.value, "Search sort order updated.")
      );
    }
  } catch (err) {
    console.error("[Manager] Failed to init settings:", err);
  }
}

// ----------------------------------------------------
// CACHE INSPECTOR
// ----------------------------------------------------
async function refreshCacheInspector() {
  if (!tauriInvoke) return;
  try {
    const stats = await tauriInvoke("get_cache_stats");
    statTotalSize.textContent = formatBytes(stats.total_size);
    statEntryCount.textContent = stats.entry_count.toLocaleString();
    statCapSize.textContent = formatBytes(stats.cap_bytes);

    const fillEl = document.querySelector("#cache-usage-fill");
    const pctEl = document.querySelector("#cache-usage-pct");
    if (fillEl && pctEl) {
      const pct = stats.cap_bytes > 0
        ? Math.min(100, Math.round((stats.total_size / stats.cap_bytes) * 100))
        : 0;
      fillEl.style.width = `${pct}%`;
      pctEl.textContent = `${pct}%`;
    }

    await loadCacheEntries();
  } catch (err) {
    console.error("[Manager] refreshCacheInspector error:", err);
  }
}

async function loadCacheEntries(filter = "") {
  if (!tauriInvoke) return;
  try {
    const entries = await tauriInvoke("list_cache_entries", {
      filter: filter || null,
      limit: 100,
      offset: 0,
    });

    cacheEntriesBody.innerHTML = "";
    if (!entries || entries.length === 0) {
      cacheEntriesBody.innerHTML = "<tr><td colspan='4' style='text-align: center; color: #94a3b8;'>No cached entries found.</td></tr>";
      return;
    }

    for (const item of entries) {
      const tr = document.createElement("tr");

      let badgeClass = "badge-other";
      let typeLabel = "FILE";
      const ct = (item.content_type || "").toLowerCase();
      if (ct.includes("html")) { badgeClass = "badge-html"; typeLabel = "HTML"; }
      else if (ct.includes("css")) { badgeClass = "badge-css"; typeLabel = "CSS"; }
      else if (ct.includes("javascript")) { badgeClass = "badge-js"; typeLabel = "JS"; }
      else if (ct.includes("image") || ct.includes("audio") || ct.includes("video")) { badgeClass = "badge-media"; typeLabel = "MEDIA"; }

      const lastUsed = new Date(item.last_used_at * 1000).toLocaleDateString();

      tr.innerHTML = `
        <td><span class="badge ${badgeClass}">${typeLabel}</span></td>
        <td title="${item.url}">${item.url}</td>
        <td>${formatBytes(item.size)}</td>
        <td>${lastUsed}</td>
      `;
      cacheEntriesBody.appendChild(tr);
    }
  } catch (err) {
    console.error("[Manager] loadCacheEntries error:", err);
  }
}

let searchDebounce = null;
cacheSearch.addEventListener("input", () => {
  clearTimeout(searchDebounce);
  searchDebounce = setTimeout(() => {
    loadCacheEntries(cacheSearch.value.trim());
  }, 250);
});

btnClearOlder.addEventListener("click", async () => {
  if (!tauriInvoke) return;
  if (!window.confirm("Clean cache entries older than 30 days?")) return;
  const count = await tauriInvoke("clear_cache_older_than", { days: 30 });
  setMessage(`Removed ${count} older cached resources.`);
  await refreshCacheInspector();
});

btnClearAll.addEventListener("click", async () => {
  if (!tauriInvoke) return;
  if (!window.confirm("Are you sure you want to clear the entire disk cache?")) return;
  await tauriInvoke("clear_cache");
  setMessage("Disk cache cleared.");
  await refreshCacheInspector();
});

btnCrawlCurrent.addEventListener("click", async () => {
  if (!tauriInvoke) return;
  const url = prompt("Enter article or page URL to save offline (defaults to current Home):", currentSettings?.home || "");
  if (!url) return;

  setMessage(`Downloading and caching ${url} + all subresources…`);
  try {
    const res = await tauriInvoke("save_page_offline", { url });
    setMessage(`Successfully saved page offline! Downloaded ${res.downloaded_count} assets (${formatBytes(res.bytes_total)}).`);
    await refreshCacheInspector();
  } catch (err) {
    setMessage(`Save offline failed: ${err}`, true);
  }
});

// Initialization
console.log("[Manager] Initializing manager window...");
const initial = await currentIndex();
renderIndex(initial);
await refreshPage();
await initSettings();
await initCloudSync();

// ----------------------------------------------------
// CLOUD SYNC
// ----------------------------------------------------
async function initCloudSync() {
  if (!tauriInvoke) return;
  const disconnectedEl = document.querySelector("#sync-disconnected");
  const connectedEl = document.querySelector("#sync-connected");
  const btnGoogle = document.querySelector("#sync-btn-google");
  const btnNcToggle = document.querySelector("#sync-btn-nextcloud-toggle");
  const ncForm = document.querySelector("#sync-nextcloud-form");
  const ncServer = document.querySelector("#nc-server");
  const ncBrowserBtn = document.querySelector("#nc-browser-login");
  const ncUsername = document.querySelector("#nc-username");
  const ncPassword = document.querySelector("#nc-password");
  const ncRemotePath = document.querySelector("#nc-remote-path");
  const ncManualBtn = document.querySelector("#nc-manual-connect");
  const providerBadge = document.querySelector("#sync-provider-badge");
  const accountLabel = document.querySelector("#sync-account-label");
  const syncNowBtn = document.querySelector("#sync-now-btn");
  const disconnectBtn = document.querySelector("#sync-disconnect-btn");
  const syncMeta = document.querySelector("#sync-meta");
  const syncError = document.querySelector("#sync-error");

  function renderSyncStatus(status) {
    if (!status || !status.connected) {
      disconnectedEl.hidden = false;
      connectedEl.hidden = true;
      return;
    }
    disconnectedEl.hidden = true;
    connectedEl.hidden = false;
    providerBadge.textContent = status.provider === "googledrive" ? "Google Drive" : "Nextcloud";
    accountLabel.textContent = status.serverUrl
      ? `${status.accountLabel || ""} @ ${status.serverUrl}`
      : (status.accountLabel || "Connected");
    const last = status.lastSyncedAt ? formatWhen(status.lastSyncedAt) : "Never";
    syncMeta.textContent = status.syncing ? "Syncing…" : `Last synced: ${last}`;
    if (status.lastError) {
      syncError.textContent = `Last sync error: ${status.lastError}`;
      syncError.hidden = false;
    } else {
      syncError.hidden = true;
    }
  }

  async function refreshSyncStatus() {
    try {
      const status = await tauriInvoke("get_sync_status");
      renderSyncStatus(status);
    } catch (err) {
      console.warn("[Manager] get_sync_status failed:", err);
    }
  }

  window.__wolOnSyncUpdated = async () => {
    const idx = await currentIndex();
    renderIndex(idx);
    await refreshSyncStatus();
  };

  btnNcToggle?.addEventListener("click", () => {
    ncForm.hidden = !ncForm.hidden;
  });

  btnGoogle?.addEventListener("click", async () => {
    btnGoogle.disabled = true;
    setMessage("Opening Google sign-in in your browser…");
    try {
      const status = await tauriInvoke("connect_google_drive");
      renderSyncStatus(status);
      const idx = await currentIndex();
      renderIndex(idx);
      await refreshPage();
      setMessage("Google Drive connected and synced!");
    } catch (err) {
      setMessage(`Google Drive sign-in failed: ${err}`, true);
    } finally {
      btnGoogle.disabled = false;
    }
  });

  ncBrowserBtn?.addEventListener("click", async () => {
    const serverUrl = ncServer.value.trim();
    if (!serverUrl) {
      setMessage("Please enter your Nextcloud server URL.", true);
      return;
    }
    ncBrowserBtn.disabled = true;
    setMessage("Opening Nextcloud sign-in in your browser…");
    try {
      const status = await tauriInvoke("connect_nextcloud_browser", {
        serverUrl,
        remotePath: ncRemotePath.value.trim() || null,
      });
      renderSyncStatus(status);
      const idx = await currentIndex();
      renderIndex(idx);
      await refreshPage();
      setMessage("Nextcloud connected and synced!");
    } catch (err) {
      setMessage(`Nextcloud sign-in failed: ${err}`, true);
    } finally {
      ncBrowserBtn.disabled = false;
    }
  });

  ncManualBtn?.addEventListener("click", async () => {
    const serverUrl = ncServer.value.trim();
    const username = ncUsername.value.trim();
    const password = ncPassword.value;
    if (!serverUrl || !username || !password) {
      setMessage("Please fill in Server URL, Username, and App Password.", true);
      return;
    }
    ncManualBtn.disabled = true;
    setMessage("Connecting to Nextcloud…");
    try {
      const status = await tauriInvoke("connect_nextcloud", {
        serverUrl,
        username,
        password,
        remotePath: ncRemotePath.value.trim() || null,
      });
      renderSyncStatus(status);
      const idx = await currentIndex();
      renderIndex(idx);
      await refreshPage();
      setMessage("Nextcloud connected and synced!");
    } catch (err) {
      setMessage(`Nextcloud connection failed: ${err}`, true);
    } finally {
      ncManualBtn.disabled = false;
    }
  });

  syncNowBtn?.addEventListener("click", async () => {
    syncNowBtn.disabled = true;
    syncMeta.textContent = "Syncing…";
    try {
      const status = await tauriInvoke("sync_now");
      renderSyncStatus(status);
      if (status.changedLocal) {
        const idx = await currentIndex();
        renderIndex(idx);
        await refreshPage();
      }
      setMessage("Sync complete.");
    } catch (err) {
      setMessage(`Sync failed: ${err}`, true);
      await refreshSyncStatus();
    } finally {
      syncNowBtn.disabled = false;
    }
  });

  disconnectBtn?.addEventListener("click", async () => {
    if (!window.confirm("Disconnect cloud sync? Your local highlights will stay on this device.")) {
      return;
    }
    try {
      const status = await tauriInvoke("disconnect_sync");
      renderSyncStatus(status);
      setMessage("Cloud sync disconnected.");
    } catch (err) {
      setMessage(`Disconnect failed: ${err}`, true);
    }
  });

  await refreshSyncStatus();
}
