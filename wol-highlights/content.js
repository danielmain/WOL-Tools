const ext = typeof browser !== "undefined" ? browser : chrome;

console.log("[WOL Highlights Content] Script injected on:", location.href);

let applying = false;
let lastKey = "";
let lastStatus = emptyStatus();
let observer = null;
let chain = Promise.resolve();

function emptyStatus() {
  return {
    href: location.href,
    title: document.title,
    hasBackup: false,
    enabled: false,
    docId: null,
    bibleLabel: null,
    applied: 0,
    notesApplied: 0,
    textareasEnabled: 0,
    missing: 0,
    skipped: 0,
  };
}

function pageLanguage(article) {
  const match = location.pathname.match(/\/lp-([a-z0-9]+)/i);
  return (match && match[1]) || article?.dataset.lang || null;
}

function documentIdFromUrl() {
  const match = location.pathname.match(/\/wol\/d\/r\d+\/lp-[a-z0-9]+\/(\d+)/i);
  if (match) return match[1];
  const article = document.querySelector("#article");
  if (article) {
    const classMatch = article.className.match(/\bdocId-(\d+)\b/);
    if (classMatch) return classMatch[1];
    if (article.dataset?.docid) return article.dataset.docid;
  }
  return null;
}

function symbolFromUrl() {
  const match = location.pathname.match(/\/wol\/(?:b|binav)\/r\d+\/lp-[a-z0-9]+\/([^/]+)\/\d+\/\d+/i);
  return match ? decodeURIComponent(match[1]).toLowerCase() : null;
}

function bibleReferenceFromUrl() {
  const match = location.pathname.match(/\/wol\/(?:b|binav)\/r\d+\/lp-[a-z0-9]+\/([^/]+)\/(\d+)\/(\d+)/i);
  return match ? { pub: decodeURIComponent(match[1]).toLowerCase(), book: match[2], chapter: match[3] } : null;
}

function symbolFromArticle(article, index) {
  const known = new Set(Object.keys(index?.bible || {}).map((key) => key.split("|")[1]));
  for (const className of article.classList) {
    if (!className.startsWith("pub-")) continue;
    const symbol = className.slice(4).toLowerCase();
    if (known.has(symbol) || known.size === 0) return symbol;
  }
  return null;
}

function pageIssueTagNumber() {
  const el = document.getElementById("issue");
  const raw = el ? String(el.value || "").trim() : "";
  if (!/^\d{6,8}$/.test(raw)) return 0;
  const padded = raw.length === 6 ? `${raw}00` : raw;
  const num = Number.parseInt(padded, 10);
  return Number.isFinite(num) && num > 0 ? num : 0;
}

function recordDocMeta(marks, docId, article) {
  if (!marks || !docId || !article) return;
  if (!marks.docMeta) marks.docMeta = {};
  const existing = marks.docMeta[docId] || {};

  let keySymbol = existing.keySymbol || null;
  if (!keySymbol) {
    for (const className of article.classList) {
      if (className.startsWith("pub-") && className.length > 4) {
        keySymbol = className.slice(4);
        break;
      }
    }
  }

  let mepsLanguage = existing.mepsLanguage ?? null;
  if (mepsLanguage === null) {
    const langSym = pageLanguage(article);
    const parsedLang = languageId(langSym);
    if (parsedLang !== null) {
      mepsLanguage = parsedLang;
    } else if (article.dataset?.mepsLang) {
      const raw = Number.parseInt(article.dataset.mepsLang, 10);
      if (Number.isFinite(raw)) mepsLanguage = raw;
    }
  }

  const title = existing.title || (document.title ? document.title.trim() : null) || null;
  const issueTagNumber = existing.issueTagNumber || pageIssueTagNumber();

  marks.docMeta[docId] = {
    keySymbol,
    issueTagNumber,
    mepsLanguage,
    title,
  };
}

function findBibleChapterKey(bible, langSymbol, pubSymbol, book, chapter) {
  if (!bible) return null;
  const symbol = String(pubSymbol || "").toLowerCase();
  const lang = languageId(langSymbol);
  const bookStr = String(book);
  const chapterStr = String(chapter);

  // 1. Direct match with lang & symbol
  if (lang !== null && symbol) {
    const directKey = `${lang}|${symbol}|${bookStr}|${chapterStr}`;
    if (bible[directKey]) return directKey;
  }

  // 2. Exact symbol, matching book and chapter (any language)
  if (symbol) {
    const matches = Object.keys(bible).filter((k) => {
      const [, storedSymbol, storedBook, storedChapter] = k.split("|");
      return storedSymbol === symbol && storedBook === bookStr && storedChapter === chapterStr;
    });
    if (matches.length === 1) return matches[0];
    if (matches.length > 1 && lang !== null) {
      const langMatch = matches.find((k) => k.startsWith(`${lang}|`));
      if (langMatch) return langMatch;
    }
    if (matches.length > 0) return matches[0];
  }

  // 3. Any symbol matching book and chapter
  const anyMatches = Object.keys(bible).filter((k) => {
    const [, , storedBook, storedChapter] = k.split("|");
    return storedBook === bookStr && storedChapter === chapterStr;
  });
  if (anyMatches.length === 1) return anyMatches[0];
  if (anyMatches.length > 1 && lang !== null) {
    const langMatch = anyMatches.find((k) => k.startsWith(`${lang}|`));
    if (langMatch) return langMatch;
  }
  if (anyMatches.length > 0) return anyMatches[0];

  return null;
}

function bibleChapter(index, langSymbol, pubSymbol, book, chapter) {
  const key = findBibleChapterKey(index?.bible, langSymbol, pubSymbol, book, chapter);
  return key ? index.bible[key] : null;
}

function applyDocument(article, paragraphs) {
  let applied = 0;
  let missing = 0;
  let skipped = 0;
  console.log(`[WOL Highlights Content] applyDocument() targeting ${Object.keys(paragraphs).length} paragraph(s)...`);
  for (const [paragraphId, ranges] of Object.entries(paragraphs)) {
    const paragraph = article.querySelector(`[data-pid="${CSS.escape(paragraphId)}"]`) || article.querySelector(`p[data-pid="${paragraphId}"]`);
    if (!paragraph) {
      console.warn(`[WOL Highlights Content] Paragraph [pid=${paragraphId}] not found in article DOM! Skipping ${ranges.length} range(s).`);
      missing += ranges.length;
      continue;
    }
    const result = highlightBlock(paragraph, ranges);
    console.log(`[WOL Highlights Content] Paragraph [pid=${paragraphId}]: applied ${result.applied}, skipped ${result.skipped} (out of ${ranges.length} ranges)`);
    applied += result.applied;
    skipped += result.skipped;
  }
  console.log(`[WOL Highlights Content] applyDocument() finished: applied=${applied}, missing=${missing}, skipped=${skipped}`);
  return { applied, missing, skipped };
}

function applyBible(article, index) {
  const symbol = symbolFromUrl() || symbolFromArticle(article, index);
  if (!symbol) return { applied: 0, missing: 0, skipped: 0, label: null };
  console.log(`[WOL Highlights Content] applyBible() detected Bible symbol: ${symbol}`);
  const langSymbol = pageLanguage(article);
  const groups = new Map();
  for (const span of article.querySelectorAll("span.v[id]")) {
    const match = span.id.match(/^v(\d+)-(\d+)-(\d+)-\d+$/);
    if (!match) continue;
    const [, book, chapter, verse] = match;
    const groupKey = `${book}-${chapter}`;
    let group = groups.get(groupKey);
    if (!group) {
      group = { book, chapter, verses: new Map() };
      groups.set(groupKey, group);
    }
    const parts = group.verses.get(verse) || [];
    parts.push(span);
    group.verses.set(verse, parts);
  }
  let applied = 0;
  let missing = 0;
  let skipped = 0;
  const labels = [];
  for (const group of groups.values()) {
    const verses = bibleChapter(index, langSymbol, symbol, group.book, group.chapter);
    if (!verses) continue;
    labels.push(`${symbol} ${group.book}:${group.chapter}`);
    for (const [verse, ranges] of Object.entries(verses)) {
      const parts = group.verses.get(verse);
      if (!parts) {
        missing += ranges.length;
        continue;
      }
      const result = highlightBlock(parts, ranges);
      applied += result.applied;
      skipped += result.skipped;
    }
  }
  return { applied, missing, skipped, label: labels.join(", ") || null };
}

function articleKey(article) {
  if (!article) return location.href;
  const paragraphs = article.querySelectorAll("p[data-pid]").length;
  const verses = article.querySelectorAll("span.v[id]").length;
  return `${location.href}|${paragraphs}|${verses}`;
}

function apply(force = false) {
  const job = chain.then(() => applyNow(force));
  chain = job.then(() => {}, () => {});
  return job;
}

async function applyNow(force = false) {
  const article = document.querySelector("#article");
  const key = articleKey(article);
  let stored;
  try {
    stored = await ext.storage.local.get("wolMarks");
  } catch (err) {
    console.error("[WOL Highlights Content] Failed to read from storage.local:", err);
    throw err;
  }
  const index = stored?.wolMarks;
  const hasBackup = Boolean(index && index.version === 1);
  const want = hasBackup && index.enabled !== false;
  const hasMarks = Boolean(article?.querySelector("mark.wol-hl"));

  console.log("[WOL Highlights Content] applyNow() triggered:", {
    force,
    hasArticle: Boolean(article),
    hasBackup,
    enabled: want,
    currentMarksInDOM: hasMarks,
    backupInfo: hasBackup ? {
      highlights: index.highlightCount,
      documents: Object.keys(index.docs || {}).length,
      bibleChapters: Object.keys(index.bible || {}).length,
      fileName: index.fileName
    } : null
  });

  if (!force && key === lastKey && (!want || hasMarks) && (want || !hasMarks)) {
    console.log("[WOL Highlights Content] Skipped applyNow() - DOM and state unchanged.");
    return lastStatus;
  }

  applying = true;
  observer?.disconnect();
  try {
    lastKey = key;
    lastStatus = emptyStatus();
    lastStatus.hasBackup = hasBackup;
    lastStatus.enabled = want;
    document.documentElement.dataset.wolHighlights = !hasBackup ? "empty" : want ? "on" : "off";
    if (article) {
      clearMarks(article);
      clearNoteCards(article);
    }

    const docId = documentIdFromUrl();
    lastStatus.docId = docId;
    console.log("[WOL Highlights Content] Page inspection:", {
      docId,
      hasArticleElement: Boolean(article),
      totalParagraphsInDOM: article ? article.querySelectorAll("p[data-pid]").length : 0,
      hasBackup,
      enabled: want,
    });

    if (!article) {
      console.warn("[WOL Highlights Content] #article element not found in DOM.");
      return lastStatus;
    }

    // Always unlock and bind paragraph/study question textareas
    const textareaResult = enableAndBindTextareas(article, want ? index : null, docId);
    lastStatus.textareasEnabled = textareaResult.enabledCount;
    lastStatus.notesApplied = textareaResult.notesApplied;
    if (want) {
      lastStatus.notesApplied += renderBlockNotes(article, index, docId, textareaResult.consumedNotePids);
    }

    if (!hasBackup) {
      console.log("[WOL Highlights Content] Skipping highlight application: No backup is stored in browser local storage.");
      return lastStatus;
    }
    if (!want) {
      console.log("[WOL Highlights Content] Skipping highlight application: Highlights are toggled off by user.");
      return lastStatus;
    }

    console.log("[WOL Highlights Content] Target document ID:", docId);
    if (docId) {
      if (index.docs && index.docs[docId]) {
        console.log(`[WOL Highlights Content] Found ${Object.keys(index.docs[docId]).length} paragraph(s) in backup for docId=${docId}! Paragraph IDs:`, Object.keys(index.docs[docId]));
        const result = applyDocument(article, index.docs[docId]);
        lastStatus.applied += result.applied;
        lastStatus.missing += result.missing;
        lastStatus.skipped += result.skipped;
      } else {
        const sampleDocs = Object.keys(index.docs || {}).slice(0, 5);
        console.log(`[WOL Highlights Content] No highlights for docId=${docId} in loaded backup. (Total documents in backup: ${Object.keys(index.docs || {}).length}. Sample doc IDs: ${sampleDocs.join(", ")})`);
      }
    }
    const bible = applyBible(article, index);
    lastStatus.bibleLabel = bible.label;
    lastStatus.applied += bible.applied;
    lastStatus.missing += bible.missing;
    lastStatus.skipped += bible.skipped;

    console.log("[WOL Highlights Content] Completed highlight application. Summary:", lastStatus);
    return lastStatus;
  } catch (error) {
    console.error("[WOL Highlights Content] Error during applyNow():", error);
    throw error;
  } finally {
    applying = false;
    observe();
  }
}

let scheduled = false;
function schedule() {
  if (scheduled) return;
  scheduled = true;
  setTimeout(() => {
    scheduled = false;
    apply().catch((err) => {
      console.error("[WOL Highlights Content] Error in scheduled apply():", err);
    });
  }, 200);
}

function suppressAudioSelection() {
  const buttons = document.querySelectorAll("#playSelectedButton, .playSelectedButton, .playButton");
  for (const b of buttons) {
    b.remove();
  }
  const highlighted = document.querySelectorAll(".jwac-textHighlight");
  for (const el of highlighted) {
    el.classList.remove("jwac-textHighlight");
  }
}

function observe() {
  if (!observer) {
    observer = new MutationObserver((mutations) => {
      let needsApply = false;
      for (const m of mutations) {
        if (m.target instanceof Element && m.target.closest(".wol-note-card")) continue;
        if (m.type === "childList") {
          for (const node of m.addedNodes) {
            if (node instanceof Element) {
              if (
                node.id === "playSelectedButton" ||
                node.classList?.contains("playSelectedButton") ||
                node.classList?.contains("playButton")
              ) {
                node.remove();
                continue;
              }
              if (node.classList?.contains("wol-hl-toolbar") || node.classList?.contains("wol-hl")) {
                continue;
              }
              needsApply = true;
            }
          }
        } else if (m.type === "attributes") {
          if (m.attributeName === "class") {
            if (m.target instanceof Element && m.target.classList.contains("jwac-textHighlight")) {
              m.target.classList.remove("jwac-textHighlight");
            }
          } else if (m.attributeName === "disabled") {
            if (m.target instanceof HTMLTextAreaElement && m.target.disabled) {
              m.target.removeAttribute("disabled");
              m.target.disabled = false;
            }
          }
        }
      }
      if (!applying && needsApply) schedule();
    });
  }
  observer.observe(document.documentElement, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ["class", "disabled"],
  });
}

ext.storage.onChanged.addListener((changes, area) => {
  console.log("[WOL Highlights Content] ext.storage.onChanged event in area '" + area + "':", changes);
  if (area === "local" && changes.wolMarks) {
    console.log("[WOL Highlights Content] wolMarks changed! Re-applying highlights with force=true...");
    apply(true).catch((err) => {
      console.error("[WOL Highlights Content] Error re-applying highlights after storage change:", err);
    });
  }
});

ext.runtime.onMessage.addListener((message, sender, sendResponse) => {
  console.log("[WOL Highlights Content] Received runtime message:", message);
  if (message?.type === "wol-status") {
    sendResponse(lastStatus);
    return false;
  }
  if (message?.type === "wol-reapply") {
    console.log("[WOL Highlights Content] Received 'wol-reapply'. Forcing re-apply...");
    apply(true)
      .then((status) => sendResponse(status))
      .catch((err) => sendResponse({ error: err ? err.message : "Unknown error" }));
    return true; // Keep message channel open for async response (required for Chrome/Safari)
  }
  return false;
});

function startHighlights() {
  console.log("[WOL Highlights Content] Initializing content script apply()...");
  apply().catch((error) => {
    console.error("[WOL Highlights Content] Initial apply() failed:", error);
    document.documentElement.dataset.wolHighlights = `error:${error && error.message}`;
  });
  observe();
  console.log("[WOL Highlights Content] MutationObserver listening for DOM changes.");
}

// The desktop webview injects this script at document start, before the DOM exists.
if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", startHighlights, { once: true });
} else {
  startHighlights();
}

// ==========================================
// Floating Selection Toolbar & Interactive Highlighting
// ==========================================
let toolbarElement = null;
let activeRange = null;
let activeMark = null;
let selectionDebounceTimer = null;

function isTouchDevice() {
  return Boolean(
    window.matchMedia?.("(pointer: coarse)")?.matches ||
      (navigator.maxTouchPoints > 0 && window.innerWidth <= 900)
  );
}

function createToolbar() {
  if (toolbarElement) return toolbarElement;
  const toolbar = document.createElement("div");
  toolbar.className = "wol-hl-toolbar";
  toolbar.hidden = true;
  // Prevent touch/pointer down on the toolbar from collapsing the active selection on mobile
  toolbar.addEventListener("pointerdown", (e) => e.preventDefault());

  for (let color = 1; color <= 6; color++) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = `wol-hl-btn wol-hl-btn-${color}`;
    btn.title = `Color ${color}`;
    btn.dataset.color = String(color);
    btn.addEventListener("mousedown", (e) => e.preventDefault());
    btn.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      applyColorToActiveSelection(color);
    });
    toolbar.appendChild(btn);
  }

  const divider = document.createElement("div");
  divider.className = "wol-hl-divider";
  toolbar.appendChild(divider);

  const removeBtn = document.createElement("button");
  removeBtn.type = "button";
  removeBtn.className = "wol-hl-btn wol-hl-btn-remove";
  removeBtn.title = "Remove highlight";
  removeBtn.innerHTML = `
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
      <line x1="18" y1="6" x2="6" y2="18"></line>
      <line x1="6" y1="6" x2="18" y2="18"></line>
    </svg>
  `;
  removeBtn.addEventListener("mousedown", (e) => e.preventDefault());
  removeBtn.addEventListener("click", (e) => {
    e.preventDefault();
    e.stopPropagation();
    removeActiveSelectionHighlight();
  });
  toolbar.appendChild(removeBtn);

  const noteBtn = document.createElement("button");
  noteBtn.type = "button";
  noteBtn.className = "wol-hl-btn wol-hl-btn-note";
  noteBtn.title = "Add a note to this paragraph";
  noteBtn.setAttribute("aria-label", noteBtn.title);
  noteBtn.textContent = "✎";
  noteBtn.addEventListener("mousedown", (e) => e.preventDefault());
  noteBtn.addEventListener("click", (e) => {
    e.preventDefault();
    e.stopPropagation();
    addNoteToActiveSelection().catch((error) => console.error("Could not save note:", error));
  });
  toolbar.appendChild(noteBtn);

  document.body.appendChild(toolbar);
  toolbarElement = toolbar;
  return toolbar;
}

async function addNoteToActiveSelection() {
  const article = document.querySelector("#article");
  const docId = documentIdFromUrl();
  const node = activeMark || activeRange?.startContainer;
  const element = node instanceof Element ? node : node?.parentElement;
  const paragraph = element?.closest("[data-pid]");
  if (!article || !docId || !paragraph || !article.contains(paragraph)) return;
  const pid = paragraph.dataset.pid;
  const content = window.prompt("Note for this paragraph:");
  if (!content || !content.trim()) return;
  const marks = await getOrInitMarks();
  recordDocMeta(marks, docId, article);
  if (!marks.notes.docs) marks.notes.docs = {};
  if (!marks.notes.docs[docId]) marks.notes.docs[docId] = {};
  const existing = marks.notes.docs[docId][pid];
  if (!Array.isArray(existing)) marks.notes.docs[docId][pid] = existing ? [existing] : [];
  const now = new Date().toISOString();
  marks.notes.docs[docId][pid].push({
    guid: generateUUID(), title: null, content: content.trim(), created: now,
    lastModified: now, blockType: 1, blockIdentifier: Number(pid),
    userMarkId: null, locationId: null,
  });
  recalculateMetadata(marks);
  hideToolbar();
  await ext.storage.local.set({ wolMarks: marks });
  await apply(true);
}

function showToolbar(range) {
  const toolbar = createToolbar();
  activeRange = range.cloneRange();
  const rect = range.getBoundingClientRect();
  if (rect.width === 0 && rect.height === 0) {
    hideToolbar();
    return;
  }

  if (isTouchDevice()) {
    toolbar.classList.add("wol-hl-toolbar-mobile");
    toolbar.style.top = "";
    toolbar.style.left = "";
    toolbar.hidden = false;
    return;
  }

  toolbar.classList.remove("wol-hl-toolbar-mobile");
  let top = window.scrollY + rect.top;
  let left = window.scrollX + rect.left + rect.width / 2;

  if (rect.top < 65) {
    top = window.scrollY + rect.bottom + 45;
  }

  const minLeft = 110;
  const maxLeft = Math.max(minLeft, document.documentElement.clientWidth - 110);
  left = Math.max(minLeft, Math.min(left, maxLeft));

  toolbar.style.top = `${top}px`;
  toolbar.style.left = `${left}px`;
  toolbar.hidden = false;
}

function hideToolbar() {
  if (toolbarElement) {
    toolbarElement.hidden = true;
  }
  activeRange = null;
  activeMark = null;
}

async function getOrInitMarks() {
  let stored;
  try {
    stored = await ext.storage.local.get("wolMarks");
  } catch (err) {
    console.error("[WOL Highlights Content] Failed to read storage.local:", err);
  }
  let marks = stored?.wolMarks;
  if (!marks || marks.version !== 1) {
    marks = {
      version: 1,
      enabled: true,
      fileName: "My Highlights",
      loadedAt: new Date().toISOString(),
      highlightCount: 0,
      noteCount: 0,
      documentCount: 0,
      bibleChapterCount: 0,
      colorCounts: {},
      docs: {},
      bible: {},
      docMeta: {},
      notes: { docs: {}, bible: {} },
      inputFields: { docs: {}, bible: {} },
    };
  }
  if (!marks.docs) marks.docs = {};
  if (!marks.bible) marks.bible = {};
  if (!marks.docMeta) marks.docMeta = {};
  if (!marks.notes) marks.notes = { docs: {}, bible: {} };
  if (!marks.inputFields) marks.inputFields = { docs: {}, bible: {} };
  if (!marks.colorCounts) marks.colorCounts = {};
  marks.enabled = true;
  return marks;
}

function recalculateMetadata(marks) {
  if (!marks) return;
  if (!marks.docs) marks.docs = {};
  if (!marks.bible) marks.bible = {};
  if (!marks.notes) marks.notes = { docs: {}, bible: {} };
  if (!marks.colorCounts) marks.colorCounts = {};

  let highlightCount = 0;
  const colorCounts = {};
  let documentCount = 0;
  let bibleChapterCount = 0;

  for (const [docId, paragraphs] of Object.entries(marks.docs)) {
    let docHasHighlights = false;
    for (const [pid, ranges] of Object.entries(paragraphs)) {
      if (!ranges || !ranges.length) {
        delete paragraphs[pid];
        continue;
      }
      docHasHighlights = true;
      highlightCount += ranges.length;
      for (const [, , color] of ranges) {
        colorCounts[color] = (colorCounts[color] || 0) + 1;
      }
    }
    if (docHasHighlights) documentCount++;
    else delete marks.docs[docId];
  }

  for (const [key, verses] of Object.entries(marks.bible)) {
    let chHasHighlights = false;
    for (const [v, ranges] of Object.entries(verses)) {
      if (!ranges || !ranges.length) {
        delete verses[v];
        continue;
      }
      chHasHighlights = true;
      highlightCount += ranges.length;
      for (const [, , color] of ranges) {
        colorCounts[color] = (colorCounts[color] || 0) + 1;
      }
    }
    if (chHasHighlights) bibleChapterCount++;
    else delete marks.bible[key];
  }

  let noteCount = 0;
  if (marks.notes?.docs) {
    for (const [docId, pids] of Object.entries(marks.notes.docs)) {
      let docHasNotes = false;
      for (const [pid, list] of Object.entries(pids)) {
        if (Array.isArray(list) ? list.length > 0 : Boolean(list)) {
          docHasNotes = true;
          noteCount += Array.isArray(list) ? list.length : 1;
        } else {
          delete pids[pid];
        }
      }
      if (!docHasNotes) delete marks.notes.docs[docId];
    }
  }

  if (marks.notes?.bible) {
    for (const [chKey, verses] of Object.entries(marks.notes.bible)) {
      let chHasNotes = false;
      for (const [v, list] of Object.entries(verses)) {
        if (Array.isArray(list) ? list.length > 0 : Boolean(list)) {
          chHasNotes = true;
          noteCount += Array.isArray(list) ? list.length : 1;
        } else {
          delete verses[v];
        }
      }
      if (!chHasNotes) delete marks.notes.bible[chKey];
    }
  }

  if (marks.inputFields?.docs) {
    for (const [docId, fields] of Object.entries(marks.inputFields.docs)) {
      let docHasFields = false;
      for (const [tag, val] of Object.entries(fields)) {
        if (val && String(val).trim()) {
          docHasFields = true;
          noteCount++;
        } else {
          delete fields[tag];
        }
      }
      if (!docHasFields) delete marks.inputFields.docs[docId];
    }
  }

  if (marks.inputFields?.bible) {
    for (const [chKey, fields] of Object.entries(marks.inputFields.bible)) {
      let chHasFields = false;
      for (const [tag, val] of Object.entries(fields)) {
        if (val && String(val).trim()) {
          chHasFields = true;
          noteCount++;
        } else {
          delete fields[tag];
        }
      }
      if (!chHasFields) delete marks.inputFields.bible[chKey];
    }
  }

  marks.highlightCount = highlightCount;
  marks.noteCount = noteCount;
  marks.documentCount = documentCount;
  marks.bibleChapterCount = bibleChapterCount;
  marks.colorCounts = colorCounts;
  if (!marks.fileName) marks.fileName = "My Highlights";
}

function generateUUID() {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID().toUpperCase();
  }
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === "x" ? r : (r & 0x3) | 0x8;
    return v.toString(16).toUpperCase();
  });
}

function adjustTextareaHeight(textarea) {
  if (!textarea) return;
  textarea.style.height = "auto";
  const newHeight = Math.max(52, textarea.scrollHeight);
  textarea.style.height = `${newHeight}px`;
}

function getCandidatePidsForTextarea(textarea, article) {
  const fieldContainer = textarea.closest(".gen-field, [data-pid]");
  const fieldPid = fieldContainer?.dataset.pid;

  // 1. Find preceding question or prompt paragraph
  let prevEl = fieldContainer?.previousElementSibling || textarea.previousElementSibling;
  while (prevEl && !prevEl.dataset?.pid && prevEl.previousElementSibling) {
    prevEl = prevEl.previousElementSibling;
  }
  const questionP = prevEl?.dataset?.pid ? prevEl : null;
  const questionPid = questionP?.dataset.pid;

  // 2. Find body paragraphs answering this question (linked via data-rel-pid)
  const bodyPids = [];
  if (questionPid) {
    const relParagraphs = article.querySelectorAll(`p[data-rel-pid*="${CSS.escape(questionPid)}"]`);
    for (const p of relParagraphs) {
      const relAttr = p.dataset.relPid;
      try {
        const list = JSON.parse(relAttr);
        if (Array.isArray(list) && list.map(String).includes(String(questionPid))) {
          if (p.dataset.pid) bodyPids.push(p.dataset.pid);
        }
      } catch {
        if (new RegExp(`(^|[\\[,\\s])${questionPid}([\\]\\s,]|$)`).test(relAttr)) {
          if (p.dataset.pid) bodyPids.push(p.dataset.pid);
        }
      }
    }
  }

  // 3. Fallback: check next element sibling if no body paragraphs found
  let nextEl = fieldContainer?.nextElementSibling || textarea.nextElementSibling;
  while (nextEl && !nextEl.dataset?.pid && nextEl.nextElementSibling) {
    nextEl = nextEl.nextElementSibling;
  }
  const nextPid = nextEl?.dataset?.pid;

  // Priority order: body paragraphs, question pid, field container pid, next pid
  const candidatePids = [...bodyPids];
  if (questionPid && !candidatePids.includes(questionPid)) candidatePids.push(questionPid);
  if (fieldPid && !candidatePids.includes(fieldPid)) candidatePids.push(fieldPid);
  if (nextPid && !candidatePids.includes(nextPid)) candidatePids.push(nextPid);

  return { candidatePids, questionTitle: questionP?.textContent?.trim() || "" };
}

const noteSaveTimers = new WeakMap();

function clearNoteCards(article) {
  if (!article) return;
  for (const el of article.querySelectorAll(".wol-note-card")) {
    el.remove();
  }
}

function createNoteCardElement(notes, scope, key, block) {
  const items = Array.isArray(notes) ? notes : [notes];
  const card = document.createElement("div");
  card.className = "wol-note-card";
  items.forEach((note, position) => {
    if (!note || !(note.title?.trim() || note.content?.trim())) return;
    const item = document.createElement("div");
    item.className = "wol-note-item";
    const title = document.createElement("div");
    title.className = "wol-note-card-title";
    title.textContent = note.title || "";
    const body = document.createElement("div");
    body.className = "wol-note-card-body";
    body.textContent = note.content || "";
    const edit = document.createElement("button");
    edit.type = "button";
    edit.className = "wol-note-edit-button";
    edit.setAttribute("aria-label", "Edit note");
    edit.title = "Edit note";
    edit.innerHTML = '<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4Z"/></svg>';
    edit.addEventListener("click", () => {
      const form = document.createElement("form");
      form.className = "wol-note-edit";
      const titleInput = document.createElement("input");
      titleInput.value = note.title || "";
      titleInput.placeholder = "Title (optional)";
      titleInput.setAttribute("aria-label", "Note title");
      const contentInput = document.createElement("textarea");
      contentInput.value = note.content || "";
      contentInput.placeholder = "Write your note…";
      contentInput.setAttribute("aria-label", "Note content");
      const save = document.createElement("button");
      save.type = "submit";
      save.className = "wol-note-save";
      save.textContent = "Save";
      const cancel = document.createElement("button");
      cancel.type = "button";
      cancel.className = "wol-note-cancel";
      cancel.textContent = "Cancel";
      const error = document.createElement("div");
      error.className = "wol-note-error";
      error.setAttribute("role", "alert");
      cancel.addEventListener("click", () => form.replaceWith(item));
      form.addEventListener("submit", async (event) => {
        event.preventDefault();
        save.disabled = true;
        try {
          const marks = await getOrInitMarks();
          const stored = marks.notes?.[scope]?.[key]?.[block];
          const list = Array.isArray(stored) ? stored : [stored];
          const target = note.guid ? list.find((entry) => entry?.guid === note.guid) : list[position];
          if (!target) throw new Error("This note is no longer available. Reload the page and try again.");
          target.title = titleInput.value || null;
          target.content = contentInput.value;
          target.lastModified = new Date().toISOString();
          recalculateMetadata(marks);
          await ext.storage.local.set({ wolMarks: marks });
        } catch (err) {
          error.textContent = err.message || "Could not save the note.";
          save.disabled = false;
        }
      });
      form.append(titleInput, contentInput, save, cancel, error);
      item.replaceWith(form);
      contentInput.focus();
    });
    item.append(title, body, edit);
    card.appendChild(item);
  });
  return card.childElementCount ? card : null;
}

function renderBlockNotes(article, index, docId, consumedNotePids) {
  if (!article || !index?.notes) return 0;
  let rendered = 0;

  if (docId && index.notes.docs?.[docId]) {
    for (const [pid, notes] of Object.entries(index.notes.docs[docId])) {
      if (consumedNotePids?.has(String(pid))) continue;
      const target = article.querySelector(`[data-pid="${CSS.escape(String(pid))}"]`);
      if (!target) continue;
      const card = createNoteCardElement(notes, "docs", docId, pid);
      if (card) {
        target.insertAdjacentElement("afterend", card);
        rendered += 1;
      }
    }
  }

  const ref = bibleReferenceFromUrl();
  if (ref && index.notes.bible) {
    const key = findBibleChapterKey(index.notes.bible, pageLanguage(article), ref.pub, ref.book, ref.chapter);
    if (key && index.notes.bible[key]) {
      for (const [verse, notes] of Object.entries(index.notes.bible[key])) {
        const spans = article.querySelectorAll(`span.v[id^="v${ref.book}-${ref.chapter}-${verse}-"]`);
        const target = spans.length ? spans[spans.length - 1] : null;
        if (!target) continue;
        const card = createNoteCardElement(notes, "bible", key, verse);
        if (card) {
          target.insertAdjacentElement("afterend", card);
          rendered += 1;
        }
      }
    }
  }

  return rendered;
}

function enableAndBindTextareas(article, index, docId) {
  if (!article) return { enabledCount: 0, notesApplied: 0, consumedNotePids: new Set() };
  const textareas = article.querySelectorAll("textarea");
  let enabledCount = 0;
  let notesApplied = 0;
  const consumedNotePids = new Set();


  for (const textarea of textareas) {
    // 1. Enable textarea unconditionally
    if (textarea.hasAttribute("disabled") || textarea.disabled) {
      textarea.removeAttribute("disabled");
      textarea.disabled = false;
    }
    textarea.classList.add("wol-note-field");
    enabledCount++;

    const { candidatePids, questionTitle } = getCandidatePidsForTextarea(textarea, article);

    // 2. Look for existing text in backup
    let foundText = null;
    let matchedPid = null;

    // 2a. Direct match in InputField table by textarea.id (e.g. tt47, tt55, etc.)
    if (textarea.id && index?.inputFields?.docs?.[docId]) {
      const val = index.inputFields.docs[docId][textarea.id];
      if (typeof val === "string" && val.trim()) {
        foundText = val;
      }
    }


    if (foundText !== null) {
      if (document.activeElement !== textarea && textarea.value !== foundText) {
        textarea.value = foundText;
      }
      textarea.classList.toggle("wol-has-note", Boolean(foundText.trim()));
      textarea.dataset.wolPid = matchedPid || candidatePids[0] || "";
      if (foundText.trim()) {
        notesApplied++;
        if (matchedPid) consumedNotePids.add(String(matchedPid));
        console.log(`[WOL Highlights Content] Populated textarea#${textarea.id} (docId=${docId}, pid=${textarea.dataset.wolPid}) with text: "${foundText.slice(0, 30)}${foundText.length > 30 ? "..." : ""}"`);
      }
      adjustTextareaHeight(textarea);
    } else {
      if (document.activeElement !== textarea && !textarea.dataset.userEdited) {
        textarea.value = "";
        textarea.classList.remove("wol-has-note");
      }
      textarea.dataset.wolPid = candidatePids[0] || "";
      adjustTextareaHeight(textarea);
    }

    // 3. Attach listeners (only once per textarea)
    if (!textarea.dataset.wolBound) {
      textarea.dataset.wolBound = "true";

      const commitSave = () => {
        const timer = noteSaveTimers.get(textarea);
        if (timer) {
          clearTimeout(timer);
          noteSaveTimers.delete(textarea);
        }
        saveTextareaNote(textarea, candidatePids, questionTitle).catch((err) => {
          console.error("[WOL Highlights Content] Error saving note:", err);
        });
      };

      textarea.addEventListener("input", () => {
        textarea.dataset.userEdited = "true";
        adjustTextareaHeight(textarea);
        textarea.classList.toggle("wol-has-note", Boolean(textarea.value.trim()));

        const existingTimer = noteSaveTimers.get(textarea);
        if (existingTimer) clearTimeout(existingTimer);
        const newTimer = setTimeout(commitSave, 350);
        noteSaveTimers.set(textarea, newTimer);
      });

      textarea.addEventListener("blur", commitSave);
      textarea.addEventListener("change", commitSave);
    }
  }

  return { enabledCount, notesApplied, consumedNotePids };
}

async function saveTextareaNote(textarea, candidatePids, questionTitle) {
  const docId = documentIdFromUrl();
  if (!docId) return;

  const marks = await getOrInitMarks();
  recordDocMeta(marks, docId, document.querySelector("#article"));
  if (!marks.notes) marks.notes = { docs: {}, bible: {} };
  if (!marks.notes.docs) marks.notes.docs = {};
  if (!marks.notes.docs[docId]) marks.notes.docs[docId] = {};
  if (!marks.inputFields) marks.inputFields = { docs: {}, bible: {} };
  if (!marks.inputFields.docs) marks.inputFields.docs = {};
  if (!marks.inputFields.docs[docId]) marks.inputFields.docs[docId] = {};

  const text = textarea.value;
  let targetPid = textarea.dataset.wolPid;

  // 1. Save in inputFields by textarea.id (matches JW Library's InputField table; do not duplicate as Note)
  if (textarea.id) {
    if (text.trim()) {
      marks.inputFields.docs[docId][textarea.id] = text;
      textarea.classList.add("wol-has-note");
    } else {
      delete marks.inputFields.docs[docId][textarea.id];
      if (Object.keys(marks.inputFields.docs[docId]).length === 0) {
        delete marks.inputFields.docs[docId];
      }
      textarea.classList.remove("wol-has-note");
      delete textarea.dataset.userEdited;
    }
  } else {
    // 2. Fallback for textareas without an id: save as a Note by candidate pid
    const docMap = marks.notes.docs[docId];
    if (!targetPid || !candidatePids.includes(targetPid)) {
      targetPid = candidatePids.find((p) => docMap[p]) || candidatePids[0];
    }

    if (targetPid) {
      if (!text.trim()) {
        delete docMap[targetPid];
        if (Object.keys(docMap).length === 0) {
          delete marks.notes.docs[docId];
        }
        textarea.classList.remove("wol-has-note");
        delete textarea.dataset.userEdited;
      } else {
        let noteList = docMap[targetPid];
        if (Array.isArray(noteList) && noteList.length > 0) {
          noteList[0].content = text;
          noteList[0].lastModified = new Date().toISOString();
          if (!noteList[0].title && questionTitle) {
            noteList[0].title = questionTitle.slice(0, 255);
          }
        } else if (noteList && typeof noteList === "object" && !Array.isArray(noteList)) {
          noteList.content = text;
          noteList.lastModified = new Date().toISOString();
          if (!noteList.title && questionTitle) {
            noteList.title = questionTitle.slice(0, 255);
          }
        } else {
          const newNote = {
            guid: generateUUID(),
            title: questionTitle ? questionTitle.slice(0, 255) : null,
            content: text,
            created: new Date().toISOString(),
            lastModified: new Date().toISOString(),
            blockType: 1,
            blockIdentifier: parseInt(targetPid, 10) || targetPid,
            userMarkId: null,
            locationId: null,
          };
          docMap[targetPid] = [newNote];
        }
        textarea.dataset.wolPid = targetPid;
        textarea.classList.add("wol-has-note");
      }
    }
  }

  recalculateMetadata(marks);
  await ext.storage.local.set({ wolMarks: marks });
  console.log(`[WOL Highlights Content] Successfully saved note/input for docId=${docId}, id=${textarea.id}, pid=${targetPid}. Total items: ${marks.noteCount}`);
}

async function applyColorToActiveSelection(colorIndex) {
  if ((!activeRange || activeRange.collapsed) && !activeMark) {
    hideToolbar();
    return;
  }
  const article = document.querySelector("#article");
  if (!article) {
    hideToolbar();
    return;
  }

  const range = activeRange;
  const targetMark = activeMark;
  hideToolbar();
  window.getSelection()?.removeAllRanges();

  const docId = documentIdFromUrl();
  const marks = await getOrInitMarks();
  if (docId) recordDocMeta(marks, docId, article);

  // 1. Direct color change if an existing mark was clicked
  if (targetMark) {
    const sToken = parseInt(targetMark.dataset.startToken, 10);
    const eToken = parseInt(targetMark.dataset.endToken, 10);
    if (!Number.isNaN(sToken) && !Number.isNaN(eToken)) {
      if (docId) {
        const p = targetMark.closest("[data-pid]");
        const pid = p?.dataset.pid;
        if (pid) {
          if (!marks.docs[docId]) marks.docs[docId] = {};
          const existing = marks.docs[docId][pid] || [];
          marks.docs[docId][pid] = addHighlightRange(existing, [sToken, eToken, colorIndex]);
          recalculateMetadata(marks);
          await ext.storage.local.set({ wolMarks: marks });
          await apply(true);
          return;
        }
      } else {
        const vSpan = targetMark.closest("span.v[id]");
        if (vSpan) {
          const match = vSpan.id.match(/^v(\d+)-(\d+)-(\d+)-\d+$/);
          if (match) {
            const [, book, chapter, verse] = match;
            const symbol = symbolFromUrl() || symbolFromArticle(article, marks);
            const langSymbol = pageLanguage(article);
            const lang = languageId(langSymbol) ?? 0;
            const key = findBibleChapterKey(marks.bible, langSymbol, symbol, book, chapter) || `${lang}|${symbol}|${book}|${chapter}`;
            if (!marks.bible[key]) marks.bible[key] = {};
            const existing = marks.bible[key][verse] || [];
            marks.bible[key][verse] = addHighlightRange(existing, [sToken, eToken, colorIndex]);
            recalculateMetadata(marks);
            await ext.storage.local.set({ wolMarks: marks });
            await apply(true);
            return;
          }
        }
      }
    }
  }

  // 2. Selection-based highlight creation
  if (range && docId) {
    const paragraphs = [...article.querySelectorAll("[data-pid]")];
    const targetParagraphs = paragraphs.filter((p) => rangeIntersectsNode(range, p));
    if (!targetParagraphs.length) return;

    if (!marks.docs[docId]) marks.docs[docId] = {};
    for (const p of targetParagraphs) {
      const pid = p.dataset.pid;
      const span = tokenRangeForBlock(p, range);
      if (!span) continue;
      const existing = marks.docs[docId][pid] || [];
      marks.docs[docId][pid] = addHighlightRange(existing, [span.startToken, span.endToken, colorIndex]);
    }
  } else if (range) {
    const symbol = symbolFromUrl() || symbolFromArticle(article, marks);
    if (!symbol) return;
    const langSymbol = pageLanguage(article);
    const lang = languageId(langSymbol) ?? 0;

    const allVerseSpans = [...article.querySelectorAll("span.v[id]")];
    const targetSpans = allVerseSpans.filter((s) => rangeIntersectsNode(range, s));
    if (!targetSpans.length) return;

    const verseMap = new Map();
    let detectedBook = null;
    let detectedChapter = null;

    for (const span of targetSpans) {
      const match = span.id.match(/^v(\d+)-(\d+)-(\d+)-\d+$/);
      if (!match) continue;
      const [, book, chapter, verse] = match;
      detectedBook = book;
      detectedChapter = chapter;
      if (!verseMap.has(verse)) verseMap.set(verse, []);
    }

    if (!detectedBook || !detectedChapter) return;
    const key = findBibleChapterKey(marks.bible, langSymbol, symbol, detectedBook, detectedChapter) || `${lang}|${symbol}|${detectedBook}|${detectedChapter}`;
    if (!marks.bible[key]) marks.bible[key] = {};

    for (const verse of verseMap.keys()) {
      const spans = [...article.querySelectorAll(`span.v[id^="v${detectedBook}-${detectedChapter}-${verse}-"]`)];
      const span = tokenRangeForBlock(spans, range);
      if (!span) continue;
      const existing = marks.bible[key][verse] || [];
      marks.bible[key][verse] = addHighlightRange(existing, [span.startToken, span.endToken, colorIndex]);
    }
  }

  recalculateMetadata(marks);
  await ext.storage.local.set({ wolMarks: marks });
  await apply(true);
}

async function removeActiveSelectionHighlight() {
  if ((!activeRange || activeRange.collapsed) && !activeMark) {
    hideToolbar();
    return;
  }
  const article = document.querySelector("#article");
  if (!article) {
    hideToolbar();
    return;
  }

  const range = activeRange;
  const targetMark = activeMark;
  hideToolbar();
  window.getSelection()?.removeAllRanges();
  suppressAudioSelection();

  let docId = documentIdFromUrl();
  const stored = await ext.storage.local.get("wolMarks");
  const marks = stored?.wolMarks;
  if (!marks) return;
  if (!marks.docs) marks.docs = {};
  if (!marks.bible) marks.bible = {};

  if (!docId && Object.keys(marks.docs).length === 1) {
    docId = Object.keys(marks.docs)[0];
  }

  let didRemove = false;

  // 1. Direct removal if a specific mark was clicked
  if (targetMark) {
    const sToken = parseInt(targetMark.dataset.startToken, 10);
    const eToken = parseInt(targetMark.dataset.endToken, 10);
    if (!Number.isNaN(sToken) && !Number.isNaN(eToken)) {
      if (docId && marks.docs[docId]) {
        const p = targetMark.closest("[data-pid]");
        const pid = p?.dataset.pid;
        if (pid && marks.docs[docId][pid]) {
          const updated = removeHighlightRange(marks.docs[docId][pid], [sToken, eToken]);
          if (updated.length) marks.docs[docId][pid] = updated;
          else delete marks.docs[docId][pid];
          didRemove = true;
        }
      } else {
        const vSpan = targetMark.closest("span.v[id]");
        if (vSpan) {
          const match = vSpan.id.match(/^v(\d+)-(\d+)-(\d+)-\d+$/);
          if (match) {
            const [, book, chapter, verse] = match;
            const symbol = symbolFromUrl() || symbolFromArticle(article, marks);
            const langSymbol = pageLanguage(article);
            const key = findBibleChapterKey(marks.bible, langSymbol, symbol, book, chapter);
            if (key && marks.bible[key]?.[verse]) {
              const updated = removeHighlightRange(marks.bible[key][verse], [sToken, eToken]);
              if (updated.length) marks.bible[key][verse] = updated;
              else delete marks.bible[key][verse];
              didRemove = true;
            }
          }
        }
      }
    }
  }

  // 2. Selection-based removal (or fallback if direct mark removal didn't match)
  if (!didRemove && range && docId && marks.docs[docId]) {
    const paragraphs = [...article.querySelectorAll("[data-pid]")];
    const targetParagraphs = paragraphs.filter((p) => rangeIntersectsNode(range, p));

    for (const p of targetParagraphs) {
      const pid = p.dataset.pid;
      let existing = marks.docs[docId][pid];
      if (!existing || !existing.length) continue;

      // Also remove any marks intersecting this range using their tagged token bounds
      const intersectingMarks = [...p.querySelectorAll("mark.wol-hl")].filter((m) => rangeIntersectsNode(range, m));
      for (const m of intersectingMarks) {
        const s = parseInt(m.dataset.startToken, 10);
        const e = parseInt(m.dataset.endToken, 10);
        if (!Number.isNaN(s) && !Number.isNaN(e)) {
          existing = removeHighlightRange(existing, [s, e]);
        }
      }

      const span = tokenRangeForBlock(p, range);
      if (span) {
        existing = removeHighlightRange(existing, [span.startToken, span.endToken]);
      }

      if (existing.length) marks.docs[docId][pid] = existing;
      else delete marks.docs[docId][pid];
      didRemove = true;
    }
  } else if (!didRemove && range) {
    const symbol = symbolFromUrl() || symbolFromArticle(article, marks);
    if (symbol && marks.bible) {
      const langSymbol = pageLanguage(article);
      const allVerseSpans = [...article.querySelectorAll("span.v[id]")];
      const targetSpans = allVerseSpans.filter((s) => rangeIntersectsNode(range, s));

      const processed = new Set();
      for (const span of targetSpans) {
        const match = span.id.match(/^v(\d+)-(\d+)-(\d+)-\d+$/);
        if (!match) continue;
        const [, book, chapter, verse] = match;
        const key = findBibleChapterKey(marks.bible, langSymbol, symbol, book, chapter);
        if (!key || !marks.bible[key]?.[verse]) continue;
        const verseKey = `${key}|${verse}`;
        if (processed.has(verseKey)) continue;
        processed.add(verseKey);

        let existing = marks.bible[key][verse];
        if (!existing || !existing.length) continue;

        const spans = [...article.querySelectorAll(`span.v[id^="v${book}-${chapter}-${verse}-"]`)];

        // Also remove any marks intersecting this range across verse spans
        for (const s of spans) {
          const intersectingMarks = [...s.querySelectorAll("mark.wol-hl")].filter((m) => rangeIntersectsNode(range, m));
          for (const m of intersectingMarks) {
            const st = parseInt(m.dataset.startToken, 10);
            const et = parseInt(m.dataset.endToken, 10);
            if (!Number.isNaN(st) && !Number.isNaN(et)) {
              existing = removeHighlightRange(existing, [st, et]);
            }
          }
        }

        const tokenSpan = tokenRangeForBlock(spans, range);
        if (tokenSpan) {
          existing = removeHighlightRange(existing, [tokenSpan.startToken, tokenSpan.endToken]);
        }

        if (existing.length) marks.bible[key][verse] = existing;
        else delete marks.bible[key][verse];
        didRemove = true;
      }
    }
  }

  if (didRemove) {
    recalculateMetadata(marks);
    await ext.storage.local.set({ wolMarks: marks });
    await apply(true);
  }
}

function setupSelectionListeners() {
  // Capture mousedown on marks or toolbar: prevent WOL from starting audio paragraph selection
  document.addEventListener(
    "mousedown",
    (event) => {
      const el = event.target instanceof Element ? event.target : event.target?.parentElement;
      if (toolbarElement && toolbarElement.contains(el)) return;
      if (el?.closest("textarea, .gen-field")) return;
      if (el?.closest("mark.wol-hl")) {
        event.stopPropagation();
        suppressAudioSelection();
      }
    },
    true
  );

  // Capture pointerdown on marks (for touch/hybrid devices)
  document.addEventListener(
    "pointerdown",
    (event) => {
      const el = event.target instanceof Element ? event.target : event.target?.parentElement;
      if (toolbarElement && toolbarElement.contains(el)) return;
      if (el?.closest("textarea, .gen-field")) return;
      if (el?.closest("mark.wol-hl")) {
        event.stopPropagation();
        suppressAudioSelection();
      }
    },
    true
  );

  // Capture click on marks to run BEFORE WOL's paragraph onclick and stop propagation
  document.addEventListener(
    "click",
    (event) => {
      const el = event.target instanceof Element ? event.target : event.target?.parentElement;
      if (toolbarElement && toolbarElement.contains(el)) return;
      if (el?.closest("textarea, .gen-field")) return;

      const mark = el?.closest("mark.wol-hl");
      if (mark) {
        event.stopPropagation();
        event.stopImmediatePropagation();
        event.preventDefault();
        suppressAudioSelection();

        activeMark = mark;
        const range = document.createRange();
        range.selectNodeContents(mark);
        const selection = window.getSelection();
        selection?.removeAllRanges();
        selection?.addRange(range);
        showToolbar(range);
        return;
      }

      // Suppress any stray clicks on WOL's play buttons
      if (el?.id === "playSelectedButton" || el?.closest("#playSelectedButton, .playButton")) {
        event.stopPropagation();
        event.stopImmediatePropagation();
        event.preventDefault();
        suppressAudioSelection();
      }
    },
    true
  );

  document.addEventListener("mouseup", (event) => {
    const el = event.target instanceof Element ? event.target : event.target?.parentElement;
    if (toolbarElement && toolbarElement.contains(el)) return;
    if (el?.closest("textarea, .gen-field")) {
      hideToolbar();
      return;
    }

    setTimeout(() => {
      suppressAudioSelection();
      const selection = window.getSelection();
      if (!selection || selection.isCollapsed || !selection.rangeCount) {
        if (!el?.closest("mark.wol-hl")) {
          hideToolbar();
        }
        return;
      }

      const range = selection.getRangeAt(0);
      const article = document.querySelector("#article");
      if (!article || !article.contains(range.commonAncestorContainer)) {
        hideToolbar();
        return;
      }

      if (!selection.toString().trim()) {
        hideToolbar();
        return;
      }

      showToolbar(range);
    }, 20);
  });

  document.addEventListener("keyup", (event) => {
    if (event.key === "Escape") {
      hideToolbar();
      return;
    }
    if (event.target instanceof HTMLTextAreaElement || event.target?.closest?.("textarea, .gen-field")) {
      hideToolbar();
      return;
    }
    const selection = window.getSelection();
    if (!selection || selection.isCollapsed || !selection.rangeCount) {
      hideToolbar();
      return;
    }
    const range = selection.getRangeAt(0);
    const article = document.querySelector("#article");
    if (article && article.contains(range.commonAncestorContainer) && selection.toString().trim()) {
      showToolbar(range);
    } else {
      hideToolbar();
    }
  });

  // On touch devices (e.g. iOS WKWebView), mouseup fires inconsistently with selection handles.
  document.addEventListener("selectionchange", () => {
    if (!isTouchDevice()) return;
    clearTimeout(selectionDebounceTimer);
    selectionDebounceTimer = setTimeout(() => {
      const activeEl = document.activeElement;
      if (activeEl instanceof HTMLTextAreaElement || activeEl?.closest?.("textarea, .gen-field")) {
        hideToolbar();
        return;
      }
      suppressAudioSelection();
      const selection = window.getSelection();
      if (!selection || selection.isCollapsed || !selection.rangeCount) {
        if (!activeMark) hideToolbar();
        return;
      }
      const range = selection.getRangeAt(0);
      const article = document.querySelector("#article");
      if (!article || !article.contains(range.commonAncestorContainer) || !selection.toString().trim()) {
        hideToolbar();
        return;
      }
      showToolbar(range);
    }, 180);
  });
}

setupSelectionListeners();

