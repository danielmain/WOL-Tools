// Cross-compatibility: `.jwlibrary` files written by `wol-cli` must import
// cleanly in the shipped extension parser (`wol-highlights/lib/backup.js`).
//
// The CLI is a Rust binary, so this test shells out to it:
//   WOL_BIN=/path/to/wol npm test
// Without a binary (or with WOL_BIN unset and no target/{debug,release}/wol)
// the test is skipped rather than failed, so `npm test` still works on a
// machine that only has Node.

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import vm from "node:vm";
import { createRequire } from "node:module";
import { execFileSync } from "node:child_process";

const require = createRequire(import.meta.url);
const repoRoot = path.resolve(import.meta.dirname, "../..");
const backupPath = path.join(repoRoot, "wol-highlights/lib/backup.js");
const sqlWasmJsPath = path.join(repoRoot, "wol-highlights/lib/sql-wasm.js");
const sqlWasmBinPath = path.join(repoRoot, "wol-highlights/lib/sql-wasm.wasm");
const initSqlJs = require(sqlWasmJsPath);

const WOL_BIN = (() => {
  if (process.env.WOL_BIN) return process.env.WOL_BIN;
  for (const profile of ["debug", "release"]) {
    const candidate = path.join(repoRoot, "target", profile, "wol");
    if (fs.existsSync(candidate)) return candidate;
  }
  return null;
})();

function loadBackupModule() {
  const code = fs.readFileSync(backupPath, "utf8");
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
    fetch: async () => ({ ok: false, status: 404 }),
    globalThis: {},
  };
  vm.createContext(ctx);
  vm.runInContext(code, ctx);
  return ctx;
}

/** The mark set the CLI exports; deliberately includes every record kind. */
function sampleMarks() {
  return {
    version: 1,
    highlightCount: 3,
    noteCount: 2,
    ignored: 0,
    documentCount: 1,
    bibleChapterCount: 1,
    colorCounts: { 1: 1, 3: 1, 5: 1 },
    enabled: true,
    docs: {
      2025441: {
        7: [[53, 62, 1]],
        9: [[0, 4, 3]],
      },
    },
    bible: {
      "0|nwtsty|2|5": { 9: [[0, 15, 5]] },
    },
    docMeta: {
      2025441: {
        keySymbol: "w",
        issueTagNumber: 202544100,
        mepsLanguage: 0,
        title: "Lessons for Living From a Deathbed Prophecy",
      },
    },
    notes: {
      docs: {
        2025441: {
          7: [
            {
              guid: "11111111-2222-3333-4444-555555555555",
              title: "Read again",
              content: "Compare with Genesis 49:28.",
              lastModified: "2025-06-01T10:00:00.000Z",
              created: "2025-06-01T10:00:00.000Z",
              blockType: 1,
              blockIdentifier: 7,
            },
          ],
        },
      },
      bible: {},
    },
    inputFields: {
      docs: { 2025441: { tt1: "a short answer" } },
      bible: {},
    },
  };
}

function createTempDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "wol-cli-export-"));
}

function runWol(dataDir, args) {
  return execFileSync(WOL_BIN, ["--data-dir", dataDir, "--offline", ...args], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
}

test("wol-cli .jwlibrary export imports in the extension parser", { skip: !WOL_BIN && "wol binary not built (cargo build -p wol-cli)" }, async () => {
  const dataDir = createTempDir();
  const marksPath = path.join(dataDir, "marks.json");
  const exportPath = path.join(dataDir, "UserdataBackup_test.jwlibrary");
  fs.writeFileSync(marksPath, JSON.stringify(sampleMarks()));

  try {
    runWol(dataDir, ["import", marksPath]);
    runWol(dataDir, ["export", exportPath, "--format", "jwlibrary", "--meps-lang", "0"]);

    assert.ok(fs.existsSync(exportPath), "export wrote a .jwlibrary file");
    const bytes = fs.readFileSync(exportPath);
    assert.equal(bytes.subarray(0, 4).toString("hex"), "504b0304", "export is a ZIP archive");

    // The archive must be a real (deflated) zip, so also check the end record.
    const eocd = bytes.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
    assert.ok(eocd > 0, "export has an end-of-central-directory record");

    const { parseBackup } = loadBackupModule();
    // `structuredClone` drops the vm realm's prototypes so deepEqual compares values.
    const index = structuredClone(await parseBackup(new Uint8Array(bytes), initSqlJs, sqlWasmBinPath));

    assert.equal(index.highlightCount, 3);
    assert.deepEqual(index.docs["2025441"]["7"], [[53, 62, 1]]);
    assert.deepEqual(index.docs["2025441"]["9"], [[0, 4, 3]]);
    assert.deepEqual(index.bible["0|nwtsty|2|5"]["9"], [[0, 15, 5]]);
    assert.equal(index.docMeta["2025441"].keySymbol, "w");
    assert.equal(index.docMeta["2025441"].issueTagNumber, 202544100);
    assert.equal(index.notes.docs["2025441"]["7"][0].content, "Compare with Genesis 49:28.");
    assert.equal(index.notes.docs["2025441"]["7"][0].blockIdentifier, 7);
    assert.deepEqual(index.inputFields.docs["2025441"], { tt1: "a short answer" });
  } finally {
    fs.rmSync(dataDir, { recursive: true, force: true });
  }
});

test("wol-cli marks.json export round-trips through the CLI importer", { skip: !WOL_BIN && "wol binary not built (cargo build -p wol-cli)" }, () => {
  const dataDir = createTempDir();
  const first = path.join(dataDir, "first.json");
  const second = path.join(dataDir, "second.json");
  fs.writeFileSync(first, JSON.stringify(sampleMarks()));

  try {
    runWol(dataDir, ["import", first]);
    runWol(dataDir, ["export", second, "--format", "json"]);
    const exported = JSON.parse(fs.readFileSync(second, "utf8"));

    assert.equal(exported.highlightCount, 3);
    assert.deepEqual(exported.docs["2025441"]["7"], [[53, 62, 1]]);
    assert.deepEqual(exported.bible["0|nwtsty|2|5"]["9"], [[0, 15, 5]]);
    assert.equal(exported.notes.docs["2025441"]["7"][0].content, "Compare with Genesis 49:28.");
    assert.equal(exported.inputFields.docs["2025441"].tt1, "a short answer");
    assert.equal(exported.docMeta["2025441"].keySymbol, "w");
  } finally {
    fs.rmSync(dataDir, { recursive: true, force: true });
  }
});
