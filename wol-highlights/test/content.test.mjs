import { readFileSync } from "node:fs";
import { createContext, runInContext } from "node:vm";
import { test } from "node:test";
import assert from "node:assert/strict";

test("creating a highlight saves it and repaints it even when notes are present", async () => {
  const paragraph = { dataset: { pid: "2" } };
  const article = {
    classList: [],
    dataset: {},
    querySelector: (selector) => selector.includes('data-pid="2"') ? paragraph : null,
    querySelectorAll: (selector) => selector === "[data-pid]" || selector === "p[data-pid]" ? [paragraph] : [],
  };
  let saved;
  const painted = [];
  const location = { pathname: "/en/wol/d/r1/lp-e/202026402", href: "https://wol.jw.org/en/wol/d/r1/lp-e/202026402" };
  const context = createContext({
    location,
    document: {
      title: "Test article",
      readyState: "loading",
      documentElement: { dataset: {} },
      querySelector: (selector) => selector === "#article" ? article : null,
      getElementById: () => null,
      addEventListener: () => {},
    },
    browser: {
      storage: {
        local: { get: async () => ({ wolMarks: saved }), set: async ({ wolMarks }) => { saved = wolMarks; } },
        onChanged: { addListener: () => {} },
      },
      runtime: { onMessage: { addListener: () => {} } },
    },
    window: { getSelection: () => ({ removeAllRanges() {} }) },
    CSS: { escape: String },
    console: { log() {}, warn() {}, error() {} },
    clearMarks() {},
    highlightBlock: (block, ranges) => { painted.push({ block, ranges }); return { applied: ranges.length, skipped: 0 }; },
    rangeIntersectsNode: () => true,
    tokenRangeForBlock: () => ({ startToken: 0, endToken: 1 }),
    addHighlightRange: (ranges, range) => [...ranges, range],
    languageId: () => 0,
    MutationObserver: class { observe() {} disconnect() {} },
    setTimeout,
  });
  const source = readFileSync(new URL("../content.js", import.meta.url), "utf8");
  runInContext(source, context);
  runInContext("activeRange = { collapsed: false }", context);

  await runInContext("applyColorToActiveSelection(1)", context);

  assert.equal(saved.highlightCount, 1);
  assert.ok(saved.notes);
  assert.equal(painted.length, 1);
  assert.equal(painted[0].block, paragraph);
  assert.deepEqual(Array.from(painted[0].ranges[0]), [0, 1, 1]);
});
