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

function symbolFromArticle(article, index) {
  const known = new Set(Object.keys(index?.bible || {}).map((key) => key.split("|")[1]));
  for (const className of article.classList) {
    if (!className.startsWith("pub-")) continue;
    const symbol = className.slice(4).toLowerCase();
    if (known.has(symbol) || known.size === 0) return symbol;
  }
  return null;
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
    if (article) clearMarks(article);

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
        } else if (m.type === "attributes" && m.attributeName === "class") {
          if (m.target instanceof Element && m.target.classList.contains("jwac-textHighlight")) {
            m.target.classList.remove("jwac-textHighlight");
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
    attributeFilter: ["class"],
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

console.log("[WOL Highlights Content] Initializing content script apply()...");
apply().catch((error) => {
  console.error("[WOL Highlights Content] Initial apply() failed:", error);
  document.documentElement.dataset.wolHighlights = `error:${error && error.message}`;
});
observe();
console.log("[WOL Highlights Content] MutationObserver listening for DOM changes.");

// ==========================================
// Floating Selection Toolbar & Interactive Highlighting
// ==========================================
let toolbarElement = null;
let activeRange = null;
let activeMark = null;

function createToolbar() {
  if (toolbarElement) return toolbarElement;
  const toolbar = document.createElement("div");
  toolbar.className = "wol-hl-toolbar";
  toolbar.hidden = true;

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

  document.body.appendChild(toolbar);
  toolbarElement = toolbar;
  return toolbar;
}

function showToolbar(range) {
  const toolbar = createToolbar();
  activeRange = range.cloneRange();
  const rect = range.getBoundingClientRect();
  if (rect.width === 0 && rect.height === 0) {
    hideToolbar();
    return;
  }

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
      documentCount: 0,
      bibleChapterCount: 0,
      colorCounts: {},
      docs: {},
      bible: {},
    };
  }
  if (!marks.docs) marks.docs = {};
  if (!marks.bible) marks.bible = {};
  if (!marks.colorCounts) marks.colorCounts = {};
  marks.enabled = true;
  return marks;
}

function recalculateMetadata(marks) {
  if (!marks) return;
  if (!marks.docs) marks.docs = {};
  if (!marks.bible) marks.bible = {};
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

  marks.highlightCount = highlightCount;
  marks.documentCount = documentCount;
  marks.bibleChapterCount = bibleChapterCount;
  marks.colorCounts = colorCounts;
  if (!marks.fileName) marks.fileName = "My Highlights";
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
}

setupSelectionListeners();

