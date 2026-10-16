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
  }
  return null;
}

function symbolFromUrl() {
  const match = location.pathname.match(/\/wol\/(?:b|binav)\/r\d+\/lp-[a-z0-9]+\/([^/]+)\/\d+\/\d+/i);
  return match ? decodeURIComponent(match[1]).toLowerCase() : null;
}

function symbolFromArticle(article, index) {
  const known = new Set(Object.keys(index.bible).map((key) => key.split("|")[1]));
  for (const className of article.classList) {
    if (!className.startsWith("pub-")) continue;
    const symbol = className.slice(4).toLowerCase();
    if (known.has(symbol)) return symbol;
  }
  return null;
}

function bibleChapter(index, langSymbol, pubSymbol, book, chapter) {
  const symbol = String(pubSymbol).toLowerCase();
  const lang = languageId(langSymbol);
  if (lang !== null) {
    const direct = index.bible[`${lang}|${symbol}|${book}|${chapter}`];
    if (direct) return direct;
  }
  const matches = Object.entries(index.bible).filter(([key]) => {
    const [, storedSymbol, storedBook, storedChapter] = key.split("|");
    return storedSymbol === symbol && storedBook === String(book) && storedChapter === String(chapter);
  });
  return matches.length === 1 ? matches[0][1] : null;
}

function applyDocument(article, paragraphs) {
  let applied = 0;
  let missing = 0;
  let skipped = 0;
  console.log(`[WOL Highlights Content] applyDocument() targeting ${Object.keys(paragraphs).length} paragraph(s)...`);
  for (const [paragraphId, ranges] of Object.entries(paragraphs)) {
    const paragraph = article.querySelector(`p[data-pid="${paragraphId}"]`);
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

function observe() {
  if (!observer) {
    observer = new MutationObserver(() => {
      if (!applying) schedule();
    });
  }
  observer.observe(document.documentElement, { childList: true, subtree: true });
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
