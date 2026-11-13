import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import zlib from "node:zlib";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const repoRoot = path.resolve(import.meta.dirname, "../..");
const extBackupPath = path.join(repoRoot, "wol-highlights/lib/backup.js");
const desktopBackupPath = path.join(repoRoot, "wol-desktop/ui/lib/backup.js");
const sqlWasmJsPath = path.join(repoRoot, "wol-highlights/lib/sql-wasm.js");
const sqlWasmBinPath = path.join(repoRoot, "wol-highlights/lib/sql-wasm.wasm");
const initSqlJs = require(sqlWasmJsPath);

function loadBackupModule() {
  const code = fs.readFileSync(extBackupPath, "utf8");
  const ctx = {
    console,
    performance,
    Uint8Array,
    DataView,
    TextDecoder,
    DecompressionStream,
    Blob,
    Response,
    URL,
    fetch: async (p) => ({ ok: true, arrayBuffer: async () => fs.readFileSync(p).buffer }),
    globalThis: {},
  };
  vm.createContext(ctx);
  vm.runInContext(code, ctx);
  return ctx;
}

function crc32(buf) {
  let crc = 0xffffffff;
  for (let i = 0; i < buf.length; i += 1) {
    crc ^= buf[i];
    for (let j = 0; j < 8; j += 1) {
      crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function buildZip(entries, { useDataDescriptor = false } = {}) {
  const localParts = [];
  const cdParts = [];
  let offset = 0;

  for (const entry of entries) {
    const nameBuf = Buffer.from(entry.name, "utf8");
    const rawBuf = Buffer.from(entry.data);
    const compressedBuf = zlib.deflateRawSync(rawBuf);
    const crc = crc32(rawBuf);
    const flags = useDataDescriptor ? 0x0808 : 0x0800;

    const lfh = Buffer.alloc(30);
    lfh.writeUInt32LE(0x04034b50, 0);
    lfh.writeUInt16LE(20, 4);
    lfh.writeUInt16LE(flags, 6);
    lfh.writeUInt16LE(8, 8);
    lfh.writeUInt32LE(useDataDescriptor ? 0 : crc, 14);
    lfh.writeUInt32LE(useDataDescriptor ? 0 : compressedBuf.length, 18);
    lfh.writeUInt32LE(useDataDescriptor ? 0 : rawBuf.length, 22);
    lfh.writeUInt16LE(nameBuf.length, 26);
    lfh.writeUInt16LE(0, 28);

    const parts = [lfh, nameBuf, compressedBuf];
    if (useDataDescriptor) {
      const dd = Buffer.alloc(16);
      dd.writeUInt32LE(0x08074b50, 0);
      dd.writeUInt32LE(crc, 4);
      dd.writeUInt32LE(compressedBuf.length, 8);
      dd.writeUInt32LE(rawBuf.length, 12);
      parts.push(dd);
    }
    const entryBlob = Buffer.concat(parts);
    localParts.push(entryBlob);

    const cd = Buffer.alloc(46);
    cd.writeUInt32LE(0x02014b50, 0);
    cd.writeUInt16LE(20, 4);
    cd.writeUInt16LE(20, 6);
    cd.writeUInt16LE(flags, 8);
    cd.writeUInt16LE(8, 10);
    cd.writeUInt32LE(crc, 16);
    cd.writeUInt32LE(compressedBuf.length, 20);
    cd.writeUInt32LE(rawBuf.length, 24);
    cd.writeUInt16LE(nameBuf.length, 28);
    cd.writeUInt32LE(offset, 42);
    cdParts.push(cd, nameBuf);

    offset += entryBlob.length;
  }

  const cdBuf = Buffer.concat(cdParts);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(entries.length, 8);
  eocd.writeUInt16LE(entries.length, 10);
  eocd.writeUInt32LE(cdBuf.length, 12);
  eocd.writeUInt32LE(offset, 16);

  return new Uint8Array(Buffer.concat([...localParts, cdBuf, eocd]));
}

async function createSampleUserDataDb() {
  const SQL = await initSqlJs({
    locateFile: () => sqlWasmBinPath,
  });
  const db = new SQL.Database();
  db.run(`
    PRAGMA user_version = 16;
    CREATE TABLE Location (
      LocationId INTEGER NOT NULL PRIMARY KEY,
      BookNumber INTEGER,
      ChapterNumber INTEGER,
      DocumentId INTEGER,
      Track INTEGER,
      IssueTagNumber INTEGER NOT NULL DEFAULT 0,
      KeySymbol TEXT,
      MepsLanguage INTEGER,
      Type INTEGER NOT NULL,
      Title TEXT,
      Specialty TEXT,
      Edition TEXT
    );
    CREATE TABLE UserMark (
      UserMarkId INTEGER PRIMARY KEY,
      ColorIndex INTEGER NOT NULL,
      LocationId INTEGER NOT NULL,
      StyleIndex INTEGER NOT NULL,
      UserMarkGuid TEXT NOT NULL UNIQUE,
      Version INTEGER NOT NULL
    );
    CREATE TABLE BlockRange (
      BlockRangeId INTEGER PRIMARY KEY,
      BlockType INTEGER NOT NULL,
      Identifier INTEGER NOT NULL,
      StartToken INTEGER,
      EndToken INTEGER,
      UserMarkId INTEGER NOT NULL
    );
    CREATE TABLE Note (
      NoteId INTEGER PRIMARY KEY,
      Guid TEXT NOT NULL UNIQUE,
      UserMarkId INTEGER,
      LocationId INTEGER,
      Title TEXT,
      Content TEXT,
      LastModified TEXT NOT NULL,
      Created TEXT NOT NULL,
      BlockType INTEGER NOT NULL DEFAULT 0,
      BlockIdentifier INTEGER
    );
    CREATE TABLE InputField (
      LocationId INTEGER NOT NULL,
      TextTag TEXT NOT NULL,
      Value TEXT NOT NULL,
      PRIMARY KEY (LocationId, TextTag)
    );
    CREATE TABLE grdb_migrations (identifier TEXT NOT NULL PRIMARY KEY);
    INSERT INTO grdb_migrations (identifier) VALUES ('v15'), ('v16');

    -- Location 1: mwb article with MepsLanguage=1 (UserMark + Note)
    INSERT INTO Location (LocationId, DocumentId, IssueTagNumber, KeySymbol, MepsLanguage, Type, Title)
      VALUES (1, 202026402, 20261100, 'mwb', 1, 0, '9-15 de noviembre');
    -- Location 2: same mwb article with MepsLanguage=NULL (JW Library InputField convention)
    INSERT INTO Location (LocationId, DocumentId, IssueTagNumber, KeySymbol, MepsLanguage, Type)
      VALUES (2, 202026402, 20261100, 'mwb', NULL, 0);
    -- Location 3: Bible chapter nwtsty Exodus 5
    INSERT INTO Location (LocationId, BookNumber, ChapterNumber, IssueTagNumber, KeySymbol, MepsLanguage, Type)
      VALUES (3, 2, 5, 0, 'nwtsty', 1, 0);

    INSERT INTO UserMark (UserMarkId, ColorIndex, LocationId, StyleIndex, UserMarkGuid, Version)
      VALUES (1, 5, 1, 0, 'B431F40D-6F40-42E5-9607-B8546E500D10', 1),
             (2, 1, 3, 0, '975485EA-E49B-4716-9EF7-0E0CE12F794E', 1);
    INSERT INTO BlockRange (BlockRangeId, BlockType, Identifier, StartToken, EndToken, UserMarkId)
      VALUES (1, 1, 8, 0, 2, 1),
             (2, 2, 9, 0, 15, 2);
    INSERT INTO Note (NoteId, Guid, UserMarkId, LocationId, Title, Content, LastModified, Created, BlockType, BlockIdentifier)
      VALUES (1, 'NOTE-GUID-1', NULL, 1, 'Titulo1 nota', 'Nota body', '2026-11-13T12:00:00Z', '2026-11-13T12:00:00Z', 1, 8);
    INSERT INTO InputField (LocationId, TextTag, Value)
      VALUES (2, 'tt21', 'Test10'),
             (2, 'tt26', 'Test11');
  `);
  const bytes = db.export();
  db.close();
  return bytes;
}

test("extension and desktop backup.js stay identical", () => {
  const ext = fs.readFileSync(extBackupPath, "utf8");
  const desk = fs.readFileSync(desktopBackupPath, "utf8");
  assert.equal(ext, desk);
});

test("parseBackup reads standard .jwlibrary with highlights, notes, and MepsLanguage=NULL inputFields", async () => {
  const { parseBackup } = loadBackupModule();
  const dbBytes = await createSampleUserDataDb();
  const zipBytes = buildZip([
    { name: "userData.db", data: dbBytes },
    { name: "manifest.json", data: JSON.stringify({ version: 1, type: 0 }) },
  ]);

  const index = structuredClone(await parseBackup(zipBytes, initSqlJs, sqlWasmBinPath));
  assert.equal(index.highlightCount, 2);
  assert.deepEqual(index.docs["202026402"]["8"], [[0, 2, 5]]);
  assert.deepEqual(index.bible["1|nwtsty|2|5"]["9"], [[0, 15, 1]]);
  assert.equal(index.notes.docs["202026402"]["8"][0].title, "Titulo1 nota");
  assert.equal(index.notes.docs["202026402"]["8"][0].content, "Nota body");
  assert.deepEqual(index.inputFields.docs["202026402"], {
    tt21: "Test10",
    tt26: "Test11",
  });
});

test("parseBackup reads Android ZipOutputStream .jwlibrary with data descriptors (flags & 0x08, compressedSize=0 in LFH)", async () => {
  const { parseBackup } = loadBackupModule();
  const dbBytes = await createSampleUserDataDb();
  const zipBytes = buildZip(
    [
      { name: "default_thumbnail.png", data: Buffer.from([0x89, 0x50, 0x4e, 0x47]) },
      { name: "userData.db", data: dbBytes },
      { name: "manifest.json", data: JSON.stringify({ version: 1, type: 0 }) },
    ],
    { useDataDescriptor: true }
  );

  const index = structuredClone(await parseBackup(zipBytes, initSqlJs, sqlWasmBinPath));
  assert.equal(index.highlightCount, 2);
  assert.equal(index.inputFields.docs["202026402"].tt21, "Test10");
  assert.equal(index.notes.docs["202026402"]["8"][0].content, "Nota body");
});

async function createAndroidTestUserDataDb() {
  const SQL = await initSqlJs({
    locateFile: () => sqlWasmBinPath,
  });
  const db = new SQL.Database();
  db.run(`
    PRAGMA user_version = 16;
    CREATE TABLE Location (
      LocationId INTEGER NOT NULL PRIMARY KEY,
      BookNumber INTEGER,
      ChapterNumber INTEGER,
      DocumentId INTEGER,
      Track INTEGER,
      IssueTagNumber INTEGER NOT NULL DEFAULT 0,
      KeySymbol TEXT,
      MepsLanguage INTEGER,
      Type INTEGER NOT NULL,
      Title TEXT,
      Specialty INTEGER,
      Edition TEXT
    );
    CREATE TABLE UserMark (
      UserMarkId INTEGER NOT NULL PRIMARY KEY,
      ColorIndex INTEGER NOT NULL,
      LocationId INTEGER NOT NULL,
      StyleIndex INTEGER NOT NULL,
      UserMarkGuid TEXT NOT NULL,
      Version INTEGER NOT NULL
    );
    CREATE TABLE BlockRange (
      BlockRangeId INTEGER NOT NULL PRIMARY KEY,
      BlockType INTEGER NOT NULL,
      Identifier INTEGER NOT NULL,
      StartToken INTEGER,
      EndToken INTEGER,
      UserMarkId INTEGER NOT NULL
    );
    CREATE TABLE Note (
      NoteId INTEGER NOT NULL PRIMARY KEY,
      Guid TEXT NOT NULL,
      UserMarkId INTEGER,
      LocationId INTEGER,
      Title TEXT,
      Content TEXT,
      LastModified TEXT NOT NULL,
      Created TEXT NOT NULL,
      BlockType INTEGER NOT NULL DEFAULT 0,
      BlockIdentifier INTEGER
    );
    CREATE TABLE InputField (
      LocationId INTEGER NOT NULL,
      TextTag TEXT NOT NULL,
      Value TEXT NOT NULL
    );
    INSERT INTO Location (LocationId, DocumentId, IssueTagNumber, KeySymbol, MepsLanguage, Type, Title)
      VALUES (1, 202026402, 20261100, 'mwb', 1, 0, ''),
             (2, 202026402, 20261100, 'mwb', NULL, 0, ''),
             (3, 1102025916, 0, 'wcg', 1, 0, '');
    INSERT INTO UserMark (UserMarkId, ColorIndex, LocationId, StyleIndex, UserMarkGuid, Version)
      VALUES (1, 1, 1, 0, '7bb40248-ca5e-4e6b-b5e2-b04e99b87ee5', 1),
             (2, 5, 1, 0, '9e21313f-f7e2-4aed-9dbb-685a52bfd0e1', 1),
             (3, 4, 1, 0, '9e9b1d7c-8b43-4510-8130-87a9b53e43e5', 1),
             (4, 2, 3, 0, '00e7a78d-d121-45e8-9771-6374fd6c98ae', 1);
    INSERT INTO BlockRange (BlockRangeId, BlockType, Identifier, StartToken, EndToken, UserMarkId)
      VALUES (1, 1, 7, 0, 0, 1),
             (2, 1, 8, 0, 1, 2),
             (3, 1, 9, 0, 3, 3),
             (4, 1, 4, 0, 68, 4);
    INSERT INTO Note (NoteId, Guid, UserMarkId, LocationId, Title, Content, LastModified, Created, BlockType, BlockIdentifier)
      VALUES (1, 'd61272fd-2eea-421c-9c06-4178a36cd483', NULL, 1, 'Test note', 'Body note', '2026-11-13T12:37:29Z', '2026-11-13T12:37:16Z', 1, 9);
    INSERT INTO InputField (LocationId, TextTag, Value)
      VALUES (2, 'tt21', 'Respuesta 1'),
             (2, 'tt26', 'Respuesta 2');
  `);
  const bytes = db.export();
  db.close();
  return bytes;
}

function createDomHighlightHarness() {
  const Node = {
    TEXT_NODE: 3,
    ELEMENT_NODE: 1,
    DOCUMENT_POSITION_FOLLOWING: 4,
    DOCUMENT_POSITION_PRECEDING: 2,
  };
  const NodeFilter = { SHOW_TEXT: 4, FILTER_ACCEPT: 1, FILTER_REJECT: 2 };

  class TextNode {
    constructor(val, parent, doc) {
      this.nodeType = 3;
      this.nodeValue = val;
      this.parentNode = parent;
      this.ownerDocument = doc;
    }
    get parentElement() {
      return this.parentNode;
    }
    splitText(offset) {
      const rest = new TextNode(this.nodeValue.slice(offset), this.parentNode, this.ownerDocument);
      this.nodeValue = this.nodeValue.slice(0, offset);
      const idx = this.parentNode.childNodes.indexOf(this);
      this.parentNode.childNodes.splice(idx + 1, 0, rest);
      return rest;
    }
    compareDocumentPosition(other) {
      const all = this.ownerDocument._allText();
      const a = all.indexOf(this);
      const b = all.indexOf(other);
      return a < b ? Node.DOCUMENT_POSITION_FOLLOWING : a > b ? Node.DOCUMENT_POSITION_PRECEDING : 0;
    }
  }

  class Elem {
    constructor(tag, attrs = {}, doc = null) {
      this.nodeType = 1;
      this.tagName = tag.toUpperCase();
      this.attrs = attrs;
      this.childNodes = [];
      this.parentNode = null;
      this.ownerDocument = doc;
      this.dataset = {};
      this.className = attrs.class || "";
    }
    appendChild(c) {
      if (c.parentNode) {
        const i = c.parentNode.childNodes.indexOf(c);
        if (i >= 0) c.parentNode.childNodes.splice(i, 1);
      }
      c.parentNode = this;
      this.childNodes.push(c);
      return c;
    }
    insertBefore(newNode, refNode) {
      if (newNode.parentNode) {
        const i = newNode.parentNode.childNodes.indexOf(newNode);
        if (i >= 0) newNode.parentNode.childNodes.splice(i, 1);
      }
      newNode.parentNode = this;
      const idx = this.childNodes.indexOf(refNode);
      this.childNodes.splice(idx, 0, newNode);
      return newNode;
    }
    closest(sel) {
      let cur = this;
      while (cur) {
        for (const s of sel.split(",").map((x) => x.trim())) {
          if (s.startsWith(".") && cur.className.split(/\s+/).includes(s.slice(1))) return cur;
          if (s === "a.b" && cur.tagName === "A" && cur.className.split(/\s+/).includes("b")) return cur;
          if (s === "a.vl" && cur.tagName === "A" && cur.className.split(/\s+/).includes("vl")) return cur;
          if (s === "a.cl" && cur.tagName === "A" && cur.className.split(/\s+/).includes("cl")) return cur;
          if (cur.tagName.toLowerCase() === s) return cur;
        }
        cur = cur.parentNode;
      }
      return null;
    }
    querySelectorAll(sel) {
      const out = [];
      const walk = (n) => {
        for (const c of n.childNodes) {
          if (c.nodeType === 1) {
            if (sel === "mark.wol-hl" && c.tagName === "MARK" && c.className.includes("wol-hl")) out.push(c);
            walk(c);
          }
        }
      };
      walk(this);
      return out;
    }
    get textContent() {
      return this.childNodes.map((c) => (c.nodeType === 3 ? c.nodeValue : c.textContent)).join("");
    }
    normalize() {}
    toHTML() {
      const cls = this.className ? ` class="${this.className}"` : "";
      return `<${this.tagName.toLowerCase()}${cls}>${this.childNodes
        .map((c) => (c.nodeType === 3 ? c.nodeValue : c.toHTML()))
        .join("")}</${this.tagName.toLowerCase()}>`;
    }
  }

  const doc = {
    createElement(tag) {
      return new Elem(tag, {}, doc);
    },
    createTreeWalker(root, _what, filter) {
      const nodes = [];
      const walk = (n) => {
        for (const c of n.childNodes) {
          if (c.nodeType === 3) {
            if (filter.acceptNode(c) === NodeFilter.FILTER_ACCEPT) nodes.push(c);
          } else {
            walk(c);
          }
        }
      };
      walk(root);
      let idx = 0;
      return {
        nextNode() {
          return idx < nodes.length ? nodes[idx++] : null;
        },
      };
    },
    _allText() {
      const out = [];
      const walk = (n) => {
        for (const c of n.childNodes) {
          if (c.nodeType === 3) out.push(c);
          else walk(c);
        }
      };
      walk(this._root);
      return out;
    },
  };

  const ctx = { Node, NodeFilter, console };
  vm.createContext(ctx);
  vm.runInContext(fs.readFileSync(path.join(repoRoot, "wol-highlights/lib/tokenize.js"), "utf8"), ctx);
  vm.runInContext(fs.readFileSync(path.join(repoRoot, "wol-highlights/lib/highlight.js"), "utf8"), ctx);

  return { doc, Elem, TextNode, highlightBlock: ctx.highlightBlock };
}

test("android-test.jwlibrary fixture: parses highlights, notes, inputFields and applies DOM marks on doc 202026402", async () => {
  const { parseBackup } = loadBackupModule();
  const dbBytes = await createAndroidTestUserDataDb();
  const zipBytes = buildZip(
    [
      { name: "userData.db", data: dbBytes },
      { name: "userData.db-journal", data: Buffer.alloc(0) },
      { name: "default_thumbnail.png", data: Buffer.from([0x89, 0x50, 0x4e, 0x47]) },
      { name: "manifest.json", data: JSON.stringify({ version: 1, type: 0 }) },
    ],
    { useDataDescriptor: true }
  );

  const index = structuredClone(await parseBackup(zipBytes, initSqlJs, sqlWasmBinPath));
  assert.equal(index.highlightCount, 4);
  assert.equal(index.documentCount, 2);
  assert.deepEqual(index.docs["202026402"], {
    7: [[0, 0, 1]],
    8: [[0, 1, 5]],
    9: [[0, 3, 4]],
  });
  assert.deepEqual(index.docs["1102025916"], {
    4: [[0, 68, 2]],
  });
  assert.equal(index.notes.docs["202026402"]["9"][0].title, "Test note");
  assert.equal(index.notes.docs["202026402"]["9"][0].content, "Body note");
  assert.deepEqual(index.inputFields.docs["202026402"], {
    tt21: "Respuesta 1",
    tt26: "Respuesta 2",
  });

  const { doc, Elem, TextNode, highlightBlock } = createDomHighlightHarness();
  const paragraphs = {
    7: "Sedequías no\u00a0hizo lo correcto (Jer 52:1,\u00a02; jr 29).",
    8: "Sedequías rompió el juramento que había hecho en el nombre de Dios (Jer 52:3; w17.10 23 párrs. 9,\u00a010).",
    9: "Sedequías no\u00a0pudo librarse de las consecuencias de sus actos (Jer 52:8-11).",
  };

  for (const [pid, text] of Object.entries(paragraphs)) {
    const p = new Elem("p", {}, doc);
    doc._root = p;
    p.appendChild(new TextNode(text, p, doc));
    const res = structuredClone(highlightBlock(p, index.docs["202026402"][pid]));
    assert.deepEqual(res, { applied: 1, skipped: 0 });
    const marks = p.querySelectorAll("mark.wol-hl");
    assert.equal(marks.length, 1);
    assert.equal(marks[0].className, `wol-hl wol-c${index.docs["202026402"][pid][0][2]}`);
  }
});
