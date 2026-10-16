const QUERY = `
SELECT
  l.DocumentId AS documentId,
  l.BookNumber AS bookNumber,
  l.ChapterNumber AS chapterNumber,
  l.KeySymbol AS keySymbol,
  l.MepsLanguage AS mepsLanguage,
  b.BlockType AS blockType,
  b.Identifier AS identifier,
  b.StartToken AS startToken,
  b.EndToken AS endToken,
  u.ColorIndex AS colorIndex
FROM UserMark u
JOIN Location l ON l.LocationId = u.LocationId
JOIN BlockRange b ON b.UserMarkId = u.UserMarkId
WHERE l.Type = 0
  AND b.StartToken IS NOT NULL
  AND b.EndToken IS NOT NULL
`;

function readU16(bytes, offset) {
  return bytes[offset] | (bytes[offset + 1] << 8);
}

function readU32(bytes, offset) {
  return (bytes[offset] | (bytes[offset + 1] << 8) | (bytes[offset + 2] << 16) | (bytes[offset + 3] << 24)) >>> 0;
}

function isZip(bytes) {
  return bytes.length > 4 && bytes[0] === 0x50 && bytes[1] === 0x4b && bytes[2] === 0x03 && bytes[3] === 0x04;
}

function isSqlite(bytes) {
  const signature = "SQLite format 3";
  if (bytes.length < signature.length) return false;
  for (let index = 0; index < signature.length; index += 1) {
    if (bytes[index] !== signature.charCodeAt(index)) return false;
  }
  return true;
}

async function inflateRaw(data) {
  if (typeof DecompressionStream !== "function") {
    throw new Error("This Firefox is too old to open a .jwlibrary backup. Load userData.db instead.");
  }
  const stream = new Blob([data]).stream().pipeThrough(new DecompressionStream("deflate-raw"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

async function unzipDatabase(bytes) {
  console.log("[WOL Highlights Backup] Inspecting ZIP archive (" + bytes.length + " bytes)...");
  let offset = 0;
  while (offset + 30 <= bytes.length) {
    const signature = readU32(bytes, offset);
    if (signature === 0x02014b50 || signature === 0x06054b50) break;
    if (signature !== 0x04034b50) break;
    const flags = readU16(bytes, offset + 6);
    const method = readU16(bytes, offset + 8);
    const compressedSize = readU32(bytes, offset + 18);
    const nameLength = readU16(bytes, offset + 26);
    const extraLength = readU16(bytes, offset + 28);
    const nameStart = offset + 30;
    const name = new TextDecoder("utf-8").decode(bytes.subarray(nameStart, nameStart + nameLength));
    const dataStart = nameStart + nameLength + extraLength;
    if ((flags & 0x8) && compressedSize === 0) {
      throw new Error("This backup uses a zip layout the add-on cannot read. Load userData.db instead.");
    }
    if (dataStart + compressedSize > bytes.length) throw new Error("The backup file is truncated.");
    if (/(^|\/)userData\.db$/i.test(name)) {
      console.log("[WOL Highlights Backup] Found " + name + " in ZIP (method=" + method + ", compressed=" + compressedSize + " bytes)");
      const compressed = bytes.subarray(dataStart, dataStart + compressedSize);
      if (method === 0) return new Uint8Array(compressed);
      if (method === 8) return inflateRaw(compressed);
      throw new Error(`userData.db is compressed with method ${method}, which this add-on cannot read.`);
    }
    offset = dataStart + compressedSize;
  }
  throw new Error("No userData.db inside this backup.");
}

function indexRows(rows) {
  console.log("[WOL Highlights Backup] Indexing " + rows.length + " rows from query result...");
  const docs = {};
  const bible = {};
  const colorCounts = {};
  let highlightCount = 0;
  let ignored = 0;
  for (const row of rows) {
    const start = row.startToken;
    const end = row.endToken;
    if (start === null || end === null || end < start) {
      ignored += 1;
      continue;
    }
    const span = [start, end, row.colorIndex];
    if (row.blockType === 1 && row.documentId) {
      const paragraphs = docs[row.documentId] || (docs[row.documentId] = {});
      const list = paragraphs[row.identifier] || (paragraphs[row.identifier] = []);
      list.push(span);
      highlightCount += 1;
      colorCounts[row.colorIndex] = (colorCounts[row.colorIndex] || 0) + 1;
    } else if (row.blockType === 2 && row.bookNumber && row.chapterNumber && row.keySymbol) {
      const key = `${row.mepsLanguage}|${String(row.keySymbol).toLowerCase()}|${row.bookNumber}|${row.chapterNumber}`;
      const verses = bible[key] || (bible[key] = {});
      const list = verses[row.identifier] || (verses[row.identifier] = []);
      list.push(span);
      highlightCount += 1;
      colorCounts[row.colorIndex] = (colorCounts[row.colorIndex] || 0) + 1;
    } else {
      ignored += 1;
    }
  }
  for (const group of [docs, bible]) {
    for (const blocks of Object.values(group)) {
      for (const list of Object.values(blocks)) {
        list.sort((left, right) => left[0] - right[0] || left[1] - right[1]);
      }
    }
  }
  const result = {
    version: 1,
    highlightCount,
    ignored,
    documentCount: Object.keys(docs).length,
    bibleChapterCount: Object.keys(bible).length,
    colorCounts,
    docs,
    bible,
  };
  console.log("[WOL Highlights Backup] Index built successfully:", {
    totalHighlights: highlightCount,
    documents: result.documentCount,
    bibleChapters: result.bibleChapterCount,
    ignoredRows: ignored,
    colors: colorCounts,
  });
  return result;
}

async function parseBackup(bytes, initSqlJs, wasmUrl) {
  console.log("[WOL Highlights Backup] parseBackup() starting (" + bytes.length + " bytes)...");
  const isZipFile = isZip(bytes);
  const isSqliteFile = isSqlite(bytes);
  console.log("[WOL Highlights Backup] Format check: isZip=" + isZipFile + ", isSqlite=" + isSqliteFile);
  const databaseBytes = isZipFile ? await unzipDatabase(bytes) : bytes;
  if (!isSqlite(databaseBytes)) {
    console.error("[WOL Highlights Backup] Extracted buffer is not a valid SQLite database.");
    throw new Error("That file is not a JW Library backup or a userData.db database.");
  }
  if (typeof initSqlJs !== "function") {
    console.error("[WOL Highlights Backup] initSqlJs is not a function (got " + typeof initSqlJs + "). Check sql-wasm.js loading.");
    throw new Error("The database reader failed to start.");
  }
  console.log("[WOL Highlights Backup] Initializing sql.js with locateFile => " + wasmUrl);
  const SQL = await initSqlJs({ locateFile: () => wasmUrl });
  console.log("[WOL Highlights Backup] Opening SQLite database (" + databaseBytes.length + " bytes)...");
  const database = new SQL.Database(databaseBytes);
  try {
    console.log("[WOL Highlights Backup] Executing UserMark query...");
    const statement = database.prepare(QUERY);
    const rows = [];
    while (statement.step()) rows.push(statement.getAsObject());
    statement.free();
    console.log("[WOL Highlights Backup] Query finished. Found " + rows.length + " matching rows.");
    return indexRows(rows);
  } catch (error) {
    console.error("[WOL Highlights Backup] Error while reading database:", error);
    throw new Error(`Could not read highlights from this database. ${error.message}`);
  } finally {
    database.close();
  }
}
