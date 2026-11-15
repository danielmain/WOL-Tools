#!/usr/bin/env node
// Regenerate / verify the Rust tokenizer parity fixtures.
//
// `wol-core::tokenize` must reproduce `wol-highlights/lib/tokenize.js` exactly,
// because token indices are the coordinates stored in `wolMarks` backups. This
// script runs the shipped JS tokenizer over the fixture texts and writes the
// resulting token ranges into `wol-core/tests/fixtures/tokenize_cases.json`,
// which `wol-core/tests/tokenize_parity.rs` then replays against the Rust port.
//
// Usage:
//   node wol-highlights/test/tokenize-parity.mjs            # verify (exit 1 on drift)
//   node wol-highlights/test/tokenize-parity.mjs --update   # rewrite the fixture
//   node wol-highlights/test/tokenize-parity.mjs --print "text"  # dump tokens for a string

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";

const repoRoot = path.resolve(import.meta.dirname, "../..");
const tokenizePath = path.join(repoRoot, "wol-highlights/lib/tokenize.js");
const fixturePath = path.join(repoRoot, "wol-core/tests/fixtures/tokenize_cases.json");

/** Load the shipped content-script tokenizer (it has no exports; it declares `tokenize`). */
function loadTokenizer() {
  const code = fs.readFileSync(tokenizePath, "utf8");
  const context = vm.createContext({});
  const tokenize = new vm.Script(`${code}\n;globalThis.__tokenize = tokenize;`, {
    filename: tokenizePath,
  }).runInContext(context);
  return context.__tokenize ?? tokenize;
}

const tokenize = loadTokenizer();

const args = process.argv.slice(2);
if (args[0] === "--print") {
  const text = args.slice(1).join(" ") || "";
  console.log(JSON.stringify(tokenize(text)));
  process.exit(0);
}

const update = args.includes("--update");
const fixture = JSON.parse(fs.readFileSync(fixturePath, "utf8"));

const failures = [];
for (const topic of fixture.topics) {
  const expected = tokenize(topic.text).map((token) => ({ start: token.start, end: token.end }));
  if (update) {
    topic.tokens = expected;
    continue;
  }
  if (JSON.stringify(expected) !== JSON.stringify(topic.tokens)) {
    failures.push({ name: topic.name, expected, actual: topic.tokens });
  }
}

if (update) {
  fs.writeFileSync(fixturePath, `${JSON.stringify(fixture, null, 2)}\n`);
  console.log(`updated ${path.relative(repoRoot, fixturePath)} (${fixture.topics.length} cases)`);
  process.exit(0);
}

if (failures.length > 0) {
  console.error("tokenize fixtures are stale — run with --update");
  for (const failure of failures) {
    console.error(`  ${failure.name}`);
    console.error(`    js:   ${JSON.stringify(failure.expected)}`);
    console.error(`    file: ${JSON.stringify(failure.actual)}`);
  }
  process.exit(1);
}

console.log(`tokenize fixtures match tokenize.js (${fixture.topics.length} cases)`);
