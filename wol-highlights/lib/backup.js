const QUERY = `
SELECT
  l.DocumentId AS documentId,
  l.BookNumber AS bookNumber,
  l.ChapterNumber AS chapterNumber,
  l.KeySymbol AS keySymbol,
  l.IssueTagNumber AS issueTagNumber,
  l.MepsLanguage AS mepsLanguage,
  l.Title AS title,
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

const NOTE_QUERY = `
SELECT
  n.NoteId AS noteId,
  n.Guid AS guid,
  n.UserMarkId AS userMarkId,
  n.LocationId AS locationId,
  n.Title AS title,
  n.Content AS content,
  n.LastModified AS lastModified,
  n.Created AS created,
  n.BlockType AS blockType,
  n.BlockIdentifier AS blockIdentifier,
  l.DocumentId AS documentId,
  l.BookNumber AS bookNumber,
  l.ChapterNumber AS chapterNumber,
  l.KeySymbol AS keySymbol,
  l.MepsLanguage AS mepsLanguage
FROM Note n
LEFT JOIN Location l ON l.LocationId = n.LocationId
WHERE (n.BlockType = 1 AND l.DocumentId IS NOT NULL)
   OR (n.BlockType = 2 AND l.BookNumber IS NOT NULL AND l.ChapterNumber IS NOT NULL)
   OR (l.DocumentId IS NOT NULL AND n.BlockIdentifier IS NOT NULL)
`;

const INPUT_FIELD_QUERY = `
SELECT
  i.LocationId AS locationId,
  i.TextTag AS textTag,
  i.Value AS value,
  l.DocumentId AS documentId,
  l.BookNumber AS bookNumber,
  l.ChapterNumber AS chapterNumber,
  l.KeySymbol AS keySymbol,
  l.MepsLanguage AS mepsLanguage
FROM InputField i
LEFT JOIN Location l ON l.LocationId = i.LocationId
WHERE l.DocumentId IS NOT NULL
   OR (l.BookNumber IS NOT NULL AND l.ChapterNumber IS NOT NULL)
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

async function extractZipEntry(bytes, name, method, dataStart, compressedSize) {
  if (dataStart < 0 || dataStart + compressedSize > bytes.length) {
    throw new Error("The backup file is truncated.");
  }
  console.log("[WOL Highlights Backup] Found " + name + " in ZIP (method=" + method + ", compressed=" + compressedSize + " bytes)");
  const compressed = bytes.subarray(dataStart, dataStart + compressedSize);
  if (method === 0) return new Uint8Array(compressed);
  if (method === 8) return inflateRaw(compressed);
  throw new Error(`userData.db is compressed with method ${method}, which this add-on cannot read.`);
}

function findEocdOffset(bytes) {
  const minOffset = Math.max(0, bytes.length - 65557);
  for (let offset = bytes.length - 22; offset >= minOffset; offset -= 1) {
    if (readU32(bytes, offset) === 0x06054b50) {
      const commentLength = readU16(bytes, offset + 20);
      if (offset + 22 + commentLength === bytes.length) return offset;
    }
  }
  return -1;
}

async function unzipDatabase(bytes) {
  console.log("[WOL Highlights Backup] Inspecting ZIP archive (" + bytes.length + " bytes)...");
  const decoder = new TextDecoder("utf-8");

  // Prefer the Central Directory so ZIPs with data descriptors (e.g. Android ZipOutputStream)
  // where the local file header has compressedSize=0 are read accurately.
  const eocdOffset = findEocdOffset(bytes);
  if (eocdOffset >= 0) {
    const totalEntries = readU16(bytes, eocdOffset + 10);
    const cdOffset = readU32(bytes, eocdOffset + 16);
    let pos = cdOffset;
    for (let i = 0; i < totalEntries && pos + 46 <= bytes.length; i += 1) {
      if (readU32(bytes, pos) !== 0x02014b50) break;
      const method = readU16(bytes, pos + 10);
      const compressedSize = readU32(bytes, pos + 20);
      const nameLength = readU16(bytes, pos + 28);
      const extraLength = readU16(bytes, pos + 30);
      const commentLength = readU16(bytes, pos + 32);
      const localHeaderOffset = readU32(bytes, pos + 42);
      const name = decoder.decode(bytes.subarray(pos + 46, pos + 46 + nameLength));
      if (/(^|\/)userData\.db$/i.test(name)) {
        if (localHeaderOffset + 30 > bytes.length || readU32(bytes, localHeaderOffset) !== 0x04034b50) {
          throw new Error("The backup file is truncated.");
        }
        const localNameLength = readU16(bytes, localHeaderOffset + 26);
        const localExtraLength = readU16(bytes, localHeaderOffset + 28);
        const dataStart = localHeaderOffset + 30 + localNameLength + localExtraLength;
        return extractZipEntry(bytes, name, method, dataStart, compressedSize);
      }
      pos += 46 + nameLength + extraLength + commentLength;
    }
  }

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
    const name = decoder.decode(bytes.subarray(nameStart, nameStart + nameLength));
    const dataStart = nameStart + nameLength + extraLength;
    if ((flags & 0x8) && compressedSize === 0) {
      throw new Error("This backup uses a zip layout the add-on cannot read. Load userData.db instead.");
    }
    if (/(^|\/)userData\.db$/i.test(name)) {
      return extractZipEntry(bytes, name, method, dataStart, compressedSize);
    }
    if (dataStart + compressedSize > bytes.length) throw new Error("The backup file is truncated.");
    offset = dataStart + compressedSize;
  }
  throw new Error("No userData.db inside this backup.");
}

function indexRows(rows, noteRows = [], inputFieldRows = []) {
  console.log("[WOL Highlights Backup] Indexing " + rows.length + " highlight rows, " + noteRows.length + " note rows, and " + inputFieldRows.length + " input fields...");
  const docs = {};
  const bible = {};
  const docMeta = {};
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
      if (!docMeta[row.documentId]) {
        docMeta[row.documentId] = {
          keySymbol: row.keySymbol || null,
          issueTagNumber: row.issueTagNumber || 0,
          mepsLanguage: row.mepsLanguage ?? null,
          title: row.title || null,
        };
      }
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

  const notes = {
    docs: {},
    bible: {},
  };
  let noteCount = 0;
  for (const row of noteRows) {
    const item = {
      noteId: row.noteId,
      guid: row.guid,
      userMarkId: row.userMarkId,
      locationId: row.locationId,
      title: row.title || null,
      content: row.content || "",
      lastModified: row.lastModified || null,
      created: row.created || null,
      blockType: row.blockType,
      blockIdentifier: row.blockIdentifier,
    };

    if ((row.blockType === 1 || row.blockIdentifier != null) && row.documentId) {
      const docId = String(row.documentId);
      const pid = String(row.blockIdentifier);
      const paragraphs = notes.docs[docId] || (notes.docs[docId] = {});
      const list = paragraphs[pid] || (paragraphs[pid] = []);
      list.push(item);
      noteCount += 1;
    } else if (row.blockType === 2 && row.bookNumber && row.chapterNumber && row.keySymbol && row.blockIdentifier != null) {
      const key = `${row.mepsLanguage}|${String(row.keySymbol).toLowerCase()}|${row.bookNumber}|${row.chapterNumber}`;
      const v = String(row.blockIdentifier);
      const verses = notes.bible[key] || (notes.bible[key] = {});
      const list = verses[v] || (verses[v] = []);
      list.push(item);
      noteCount += 1;
    }
  }

  const inputFields = {
    docs: {},
    bible: {},
  };
  let inputFieldCount = 0;
  for (const row of inputFieldRows) {
    if (row.documentId && row.textTag) {
      const docId = String(row.documentId);
      const tag = String(row.textTag);
      const docFields = inputFields.docs[docId] || (inputFields.docs[docId] = {});
      docFields[tag] = row.value || "";
      if (row.value && row.value.trim()) inputFieldCount++;
    } else if (row.bookNumber && row.chapterNumber && row.keySymbol && row.textTag) {
      const key = `${row.mepsLanguage}|${String(row.keySymbol).toLowerCase()}|${row.bookNumber}|${row.chapterNumber}`;
      const tag = String(row.textTag);
      const chFields = inputFields.bible[key] || (inputFields.bible[key] = {});
      chFields[tag] = row.value || "";
      if (row.value && row.value.trim()) inputFieldCount++;
    }
  }

  const totalNotes = noteCount + inputFieldCount;
  const result = {
    version: 1,
    highlightCount,
    noteCount: totalNotes,
    ignored,
    documentCount: Object.keys(docs).length,
    bibleChapterCount: Object.keys(bible).length,
    colorCounts,
    docs,
    bible,
    docMeta,
    notes,
    inputFields,
  };
  console.log("[WOL Highlights Backup] Index built successfully:", {
    totalHighlights: highlightCount,
    totalNotes: totalNotes,
    inputFields: inputFieldCount,
    documents: result.documentCount,
    bibleChapters: result.bibleChapterCount,
    ignoredRows: ignored,
    colors: colorCounts,
  });
  return result;
}

async function parseBackup(bytes, initSqlJs, wasmUrl) {
  console.log("[WOL Highlights Backup] parseBackup() starting (" + bytes.length + " bytes)...");
  const headerPreview = Array.from(bytes.subarray(0, 16)).map((b) => b.toString(16).padStart(2, "0")).join(" ");
  const asciiPreview = Array.from(bytes.subarray(0, 16)).map((b) => (b >= 32 && b <= 126 ? String.fromCharCode(b) : ".")).join("");
  console.log(`[WOL Highlights Backup] First 16 bytes: [${headerPreview}] ASCII: "${asciiPreview}"`);

  const isZipFile = isZip(bytes);
  const isSqliteFile = isSqlite(bytes);
  console.log("[WOL Highlights Backup] Format check: isZip=" + isZipFile + ", isSqlite=" + isSqliteFile);

  let databaseBytes = bytes;
  if (isZipFile) {
    console.log("[WOL Highlights Backup] File is a ZIP archive (e.g. .jwlibrary). Extracting userData.db...");
    databaseBytes = await unzipDatabase(bytes);
    console.log("[WOL Highlights Backup] Extracted database size: " + databaseBytes.length + " bytes");
  }

  if (!isSqlite(databaseBytes)) {
    console.error("[WOL Highlights Backup] Buffer is not a valid SQLite database. Expected 'SQLite format 3' header.");
    throw new Error("That file is not a valid JW Library backup or SQLite database.");
  }

  if (typeof initSqlJs !== "function") {
    console.error("[WOL Highlights Backup] initSqlJs is not a function (got " + typeof initSqlJs + "). Check sql-wasm.js loading.");
    throw new Error("The database reader (sql.js) failed to start.");
  }

  console.log("[WOL Highlights Backup] Initializing sql.js WASM from: " + wasmUrl);
  const sqlStart = performance.now();
  const SQL = await initSqlJs({ locateFile: () => wasmUrl });
  console.log(`[WOL Highlights Backup] sql.js initialized in ${(performance.now() - sqlStart).toFixed(1)}ms. Opening SQLite database (${databaseBytes.length} bytes)...`);

  const database = new SQL.Database(databaseBytes);
  try {
    console.log("[WOL Highlights Backup] Executing UserMark query on SQLite database...");
    const queryStart = performance.now();
    const statement = database.prepare(QUERY);
    const rows = [];
    while (statement.step()) rows.push(statement.getAsObject());
    statement.free();
    console.log(`[WOL Highlights Backup] UserMark query finished in ${(performance.now() - queryStart).toFixed(1)}ms. Found ${rows.length} matching rows.`);

    const noteRows = [];
    try {
      const checkTable = database.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='Note'");
      const hasNoteTable = checkTable.step();
      checkTable.free();

      if (hasNoteTable) {
        console.log("[WOL Highlights Backup] Executing Note query on SQLite database...");
        const noteStmt = database.prepare(NOTE_QUERY);
        while (noteStmt.step()) noteRows.push(noteStmt.getAsObject());
        noteStmt.free();
        console.log(`[WOL Highlights Backup] Found ${noteRows.length} matching notes.`);
      }
    } catch (noteErr) {
      console.warn("[WOL Highlights Backup] Could not query Note table:", noteErr);
    }

    const inputFieldRows = [];
    try {
      const checkInput = database.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='InputField'");
      const hasInputTable = checkInput.step();
      checkInput.free();

      if (hasInputTable) {
        console.log("[WOL Highlights Backup] Executing InputField query on SQLite database...");
        const inputStmt = database.prepare(INPUT_FIELD_QUERY);
        while (inputStmt.step()) inputFieldRows.push(inputStmt.getAsObject());
        inputStmt.free();
        console.log(`[WOL Highlights Backup] Found ${inputFieldRows.length} matching input fields.`);
      }
    } catch (inputErr) {
      console.warn("[WOL Highlights Backup] Could not query InputField table:", inputErr);
    }

    return indexRows(rows, noteRows, inputFieldRows);
  } catch (error) {
    console.error("[WOL Highlights Backup] Error while reading database tables:", error);
    throw new Error(`Could not read highlights from this database: ${error.message}`);
  } finally {
    database.close();
  }
}
