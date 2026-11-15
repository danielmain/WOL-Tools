import { decodeJwpubContent } from './lib/jwpub-crypto.mjs';

const u16 = (v, p) => v.getUint16(p, true);
const u32 = (v, p) => v.getUint32(p, true);

async function unzip(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let end = bytes.length - 22;
  while (end >= Math.max(0, bytes.length - 65557) && u32(view, end) !== 0x06054b50) end--;
  if (end < 0) throw new Error('Invalid ZIP archive');
  let pos = u32(view, end + 16);
  const count = u16(view, end + 10), result = new Map();
  if (count > 5000) throw new Error('Too many archive entries');
  for (let i = 0; i < count; i++) {
    if (pos + 46 > bytes.length || u32(view, pos) !== 0x02014b50) throw new Error('Invalid ZIP directory');
    const method = u16(view, pos + 10), size = u32(view, pos + 20), expanded = u32(view, pos + 24);
    const nl = u16(view, pos + 28), extra = u16(view, pos + 30), comment = u16(view, pos + 32), local = u32(view, pos + 42);
    if (expanded > 30000000 || local + 30 > bytes.length || u32(view, local) !== 0x04034b50) throw new Error('Invalid archive entry');
    const name = new TextDecoder().decode(bytes.subarray(pos + 46, pos + 46 + nl));
    const start = local + 30 + u16(view, local + 26) + u16(view, local + 28);
    if (start + size > bytes.length) throw new Error('Truncated archive');
    result.set(name, async () => {
      const data = bytes.slice(start, start + size);
      if (method === 0) return data;
      if (method !== 8) throw new Error(`Unsupported compression for ${name}`);
      return new Uint8Array(await new Response(new Blob([data]).stream().pipeThrough(new DecompressionStream('deflate-raw'))).arrayBuffer());
    });
    pos += 46 + nl + extra + comment;
  }
  return result;
}

const text = (bytes) => new TextDecoder().decode(bytes);

function rows(db, sql) {
  const stmt = db.prepare(sql), result = [];
  try { while (stmt.step()) result.push(stmt.getAsObject()); } finally { stmt.free(); }
  return result;
}

function tableExists(db, name) {
  return rows(db, `SELECT 1 FROM sqlite_master WHERE type='table' AND name='${name}'`).length > 0;
}

function rewriteHref(href, id, lang) {
  const docMatch = href.match(/^jwpub:\/\/p\/[^:]+:(\d+)(?:\/(\d+))?/i);
  if (docMatch) {
    return `/imported/${id}/${docMatch[1]}${docMatch[2] ? `#p${docMatch[2]}` : ''}`;
  }
  const bibleMatch = href.match(/^jwpub:\/\/b\/[^/]+\/(\d+):(\d+):(\d+)(?:-\d+:\d+:(\d+))?/i);
  if (bibleMatch && lang) {
    const [, book, chapter, startVerse, endVerse] = bibleMatch;
    const range = endVerse ? `${book}:${chapter}:${startVerse}-${book}:${chapter}:${endVerse}` : `${book}:${chapter}:${startVerse}`;
    return `/${lang.locale}/wol/b/r${lang.region}/lp-${lang.lib}/nwtsty/${book}/${chapter}#v=${range}`;
  }
  if (href.startsWith('#') || /^https:\/\/(wol\.jw\.org|www\.jw\.org)\//i.test(href)) {
    return href;
  }
  return null;
}

function sanitizeAndExtract(html, id, assets, lang) {
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, 'text/html');
  doc.querySelectorAll('script,style,iframe,object,embed,form,input,button,textarea,link,meta,base,svg,math').forEach(e => e.remove());
  for (const el of doc.body.querySelectorAll('*')) {
    for (const attr of [...el.attributes]) {
      const name = attr.name.toLowerCase();
      if (!['id', 'class', 'data-pid', 'href', 'src', 'alt', 'title', 'colspan', 'rowspan', 'dir', 'lang'].includes(name)) {
        el.removeAttribute(attr.name);
      }
    }
    if (el.hasAttribute('href')) {
      const nextHref = rewriteHref(el.getAttribute('href'), id, lang);
      if (nextHref) el.setAttribute('href', nextHref);
      else el.removeAttribute('href');
    }
    if (el.hasAttribute('src')) {
      const src = el.getAttribute('src');
      if (assets.has(src)) el.setAttribute('src', assets.get(src));
      else el.removeAttribute('src');
    }
  }
  const headingEls = [...doc.body.querySelectorAll('h2[id][data-pid]')];
  const fallbackEls = headingEls.length ? headingEls : [...doc.body.querySelectorAll('h3[id][data-pid]')];
  const subheadings = fallbackEls
    .map(el => ({ id: el.id, title: el.textContent.replace(/\s+/g, ' ').trim() }))
    .filter(h => h.id && h.title);

  return { html: doc.body.innerHTML, subheadings };
}

function pickCoverDataUrl(manifest, assets) {
  const images = Array.isArray(manifest?.publication?.images) ? manifest.publication.images : [];
  const preferred =
    images.find(img => img.type === 't' && img.width === 120) ||
    images.find(img => img.type === 't') ||
    images.find(img => img.type === 'c');
  if (preferred?.fileName && assets.has(preferred.fileName)) {
    return assets.get(preferred.fileName);
  }
  for (const [name, dataUrl] of assets) {
    if (/_sqr-120x120\.|_sqr\.|_cvr\./i.test(name)) return dataUrl;
  }
  return null;
}

function splitTocTitle(rawTitle, chapterNumber) {
  const title = (rawTitle || '').trim();
  const numMatch = title.match(/^(\d+)\.\s+(.+)$/);
  if (numMatch) {
    return { kicker: `CAPÍTULO ${numMatch[1]}`, title: numMatch[2].trim() };
  }
  const appendixMatch = title.match(/^(Ap[eé]ndice\s+[A-Z0-9]+)\.\s+(.+)$/i);
  if (appendixMatch) {
    return { kicker: appendixMatch[1].toUpperCase(), title: appendixMatch[2].trim() };
  }
  if (chapterNumber != null && Number(chapterNumber) > 0) {
    return { kicker: `CAPÍTULO ${chapterNumber}`, title };
  }
  return { kicker: null, title };
}

function buildToc(db, docs) {
  const docByDocId = new Map(docs.map(d => [Number(d.DocumentId), d]));
  const entries = [];
  const seenMepsIds = new Set();

  const coverDoc = docs.find(d => String(d.Class) === '39');
  if (coverDoc) {
    const mepsId = Number(coverDoc.MepsDocumentId);
    seenMepsIds.add(mepsId);
    entries.push({
      section: null,
      chapterId: mepsId,
      kicker: null,
      title: coverDoc.Title || String(mepsId),
    });
  }

  if (tableExists(db, 'PublicationViewItem')) {
    const items = rows(
      db,
      'SELECT PublicationViewItemId, ParentPublicationViewItemId, Title, DefaultDocumentId FROM PublicationViewItem ORDER BY PublicationViewItemId'
    );
    const byId = new Map(items.map(it => [Number(it.PublicationViewItemId), it]));
    const sectionTitles = new Map();
    for (const it of items) {
      if (Number(it.DefaultDocumentId) < 0) {
        sectionTitles.set(Number(it.PublicationViewItemId), it.Title || '');
      }
    }

    for (const it of items) {
      const docId = Number(it.DefaultDocumentId);
      if (docId < 0) continue;
      const doc = docByDocId.get(docId);
      if (!doc || doc.MepsDocumentId == null) continue;
      const mepsId = Number(doc.MepsDocumentId);
      if (seenMepsIds.has(mepsId)) continue;
      seenMepsIds.add(mepsId);

      const parentId = Number(it.ParentPublicationViewItemId);
      const parent = byId.get(parentId);
      // Skip the generic root "CONTENIDO" wrapper so Portada/Introducción sit at top level like WOL
      const section =
        parent && Number(parent.ParentPublicationViewItemId) === -1 && Number(parent.PublicationViewItemId) === 1
          ? null
          : (sectionTitles.get(parentId) || null);

      const { kicker, title } = splitTocTitle(it.Title || doc.Title, doc.ChapterNumber);
      entries.push({ section, chapterId: mepsId, kicker, title });
    }
  }

  for (const doc of docs) {
    const mepsId = Number(doc.MepsDocumentId);
    if (seenMepsIds.has(mepsId)) continue;
    // Skip standalone Index doc (Class 19) if already represented by TOC, or include if no TOC items
    if (entries.length > 0 && String(doc.Class) === '19') continue;
    const { kicker, title } = splitTocTitle(doc.Title || String(mepsId), doc.ChapterNumber);
    entries.push({ section: null, chapterId: mepsId, kicker, title });
  }

  return entries;
}

export async function importJwpub(bytes, initSqlJs) {
  const outer = await unzip(bytes);
  if (!outer.has('manifest.json') || !outer.has('contents')) throw new Error('Not a JWPUB publication');
  const manifest = JSON.parse(text(await outer.get('manifest.json')()));
  const inner = await unzip(await outer.get('contents')());
  const dbName = manifest.publication?.fileName;
  if (!inner.has(dbName)) throw new Error('Publication database is missing');
  const SQL = await initSqlJs({ locateFile: () => 'lib/sql-wasm.wasm' });
  const db = new SQL.Database(await inner.get(dbName)());
  try {
    const pub = rows(db, 'SELECT Symbol, Year, MepsLanguageIndex, IssueTagNumber, Title, ShortTitle FROM Publication LIMIT 1')[0];
    if (!pub) throw new Error('Publication metadata is missing');
    const identity = { language: Number(pub.MepsLanguageIndex), symbol: pub.Symbol, year: Number(pub.Year), issue: Number(pub.IssueTagNumber || 0) };
    const langs = await window.__TAURI_INTERNALS__.invoke('get_languages');
    const lang = langs.find(x => x.mepsId === identity.language);
    if (!lang) throw new Error('Publication language is not supported');
    const id = `${identity.language}-${identity.symbol}-${identity.year}-${identity.issue}`.replace(/[^a-z0-9_-]/gi, '_');
    const assets = new Map();
    for (const [name, read] of inner) {
      if (!/\.(png|jpe?g|gif|webp)$/i.test(name)) continue;
      const type = /\.png$/i.test(name) ? 'image/png' : /\.gif$/i.test(name) ? 'image/gif' : /\.webp$/i.test(name) ? 'image/webp' : 'image/jpeg';
      const data = await read();
      let binary = ''; for (let i = 0; i < data.length; i += 8192) binary += String.fromCharCode(...data.subarray(i, i + 8192));
      assets.set(name, `data:${type};base64,${btoa(binary)}`);
    }
    const docs = rows(db, 'SELECT DocumentId, MepsDocumentId, Class, ChapterNumber, Title, Content FROM Document WHERE MepsDocumentId IS NOT NULL AND Content IS NOT NULL ORDER BY DocumentId');
    const chapters = [];
    for (const doc of docs) {
      const html = await decodeJwpubContent(doc.Content, identity);
      const parsed = sanitizeAndExtract(html, id, assets, lang);
      chapters.push({
        id: Number(doc.MepsDocumentId),
        title: doc.Title || String(doc.MepsDocumentId),
        chapterNumber: doc.ChapterNumber != null ? Number(doc.ChapterNumber) : null,
        subheadings: parsed.subheadings,
        html: parsed.html,
      });
    }
    const toc = buildToc(db, docs);
    const coverDataUrl = pickCoverDataUrl(manifest, assets);
    return {
      id,
      title: pub.Title,
      shortTitle: pub.ShortTitle || pub.Symbol,
      symbol: pub.Symbol,
      year: identity.year,
      issueTagNumber: identity.issue,
      language: identity.language,
      locale: lang.locale,
      lib: lang.lib,
      region: lang.region,
      coverDataUrl,
      toc,
      chapters,
    };
  } finally { db.close(); }
}
