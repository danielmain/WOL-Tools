import test from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const require = createRequire(import.meta.url);
const gen = require("../../scripts/gen-oauth.js");
const WOLSync = require("../lib/sync.js");

test("resolveCredentials prefers --from-json, then WOL_GOOGLE_*, then CHROME_*", () => {
  const fromJson = gen.resolveCredentials(
    { WOL_GOOGLE_CLIENT_ID: "env-id", WOL_GOOGLE_CLIENT_SECRET: "env-secret" },
    { clientId: "json-id", clientSecret: "json-secret" }
  );
  assert.deepEqual(
    { clientId: fromJson.clientId, clientSecret: fromJson.clientSecret },
    { clientId: "json-id", clientSecret: "json-secret" }
  );

  const fromWol = gen.resolveCredentials(
    {
      WOL_GOOGLE_CLIENT_ID: "wol-id",
      WOL_GOOGLE_CLIENT_SECRET: "wol-secret",
      CHROME_CLIENT_ID: "chrome-id",
      CHROME_CLIENT_SECRET: "chrome-secret",
    },
    null
  );
  assert.equal(fromWol.clientId, "wol-id");
  assert.equal(fromWol.clientSecret, "wol-secret");

  const fromChrome = gen.resolveCredentials(
    { CHROME_CLIENT_ID: "chrome-id", CHROME_CLIENT_SECRET: "chrome-secret" },
    null
  );
  assert.equal(fromChrome.clientId, "chrome-id");
  assert.equal(fromChrome.clientSecret, "chrome-secret");

  const blank = gen.resolveCredentials({ WOL_GOOGLE_CLIENT_ID: "   " }, null);
  assert.equal(blank.clientId, "");
  assert.equal(blank.clientSecret, "");
});

test("parseEnvFile handles quotes, export prefix, comments and blanks", () => {
  const env = gen.parseEnvFile(
    ['# comment', 'export A="1 2"', "B='x'", "C=plain", "", "D="].join("\n")
  );
  assert.equal(env.A, "1 2");
  assert.equal(env.B, "x");
  assert.equal(env.C, "plain");
  assert.equal(env.D, "");
});

test("renderModule emits a frozen global without leaking into other scopes", () => {
  const source = gen.renderModule({ clientId: 'id"quoted', clientSecret: "s3cret" });
  assert.match(source, /WOL_GOOGLE_OAUTH/);
  assert.ok(source.includes('"id\\"quoted"'), "values are JSON-escaped");
  const sandbox = {};
  const fn = new Function("globalThis", "self", `${source}\nreturn globalThis.WOL_GOOGLE_OAUTH;`);
  const result = fn(sandbox, sandbox);
  assert.equal(result.clientId, 'id"quoted');
  assert.equal(result.clientSecret, "s3cret");
  assert.ok(Object.isFrozen(result));
});

test("gen-oauth writes an empty module (exit 0) and reports --check failure without creds", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "wol-oauth-"));
  const out = path.join(dir, "oauth.generated.js");
  const envFile = path.join(dir, ".env");
  fs.writeFileSync(envFile, 'CHROME_CLIENT_ID=""\n');

  const previousEnvFile = process.env.WOL_ENV_FILE;
  process.env.WOL_ENV_FILE = envFile;
  try {
    // `ambientEnv` is empty so a developer's exported credentials cannot skew the test.
    assert.equal(gen.main(["--out", out, "--check", "--quiet"], {}), 1);
    const written = fs.readFileSync(out, "utf8");
    assert.match(written, /clientId: ""/);
    assert.match(written, /clientSecret: ""/);

    assert.equal(
      gen.main(["--out", out, "--check", "--quiet"], {
        WOL_GOOGLE_CLIENT_ID: "id",
        WOL_GOOGLE_CLIENT_SECRET: "secret",
      }),
      0
    );
    assert.match(fs.readFileSync(out, "utf8"), /clientId: "id"/);
  } finally {
    if (previousEnvFile === undefined) delete process.env.WOL_ENV_FILE;
    else process.env.WOL_ENV_FILE = previousEnvFile;
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("resolveGoogleCredentials prefers storage over the bundled client", () => {
  const stored = { clientId: "stored-id", clientSecret: "stored-secret" };
  const bundled = { clientId: "bundled-id", clientSecret: "bundled-secret" };
  const picked = WOLSync.resolveGoogleCredentials(stored, bundled);
  assert.deepEqual(
    { clientId: picked.clientId, clientSecret: picked.clientSecret, source: picked.source },
    { clientId: "stored-id", clientSecret: "stored-secret", source: "storage" }
  );

  const fallback = WOLSync.resolveGoogleCredentials(null, bundled);
  assert.equal(fallback.clientId, "bundled-id");
  assert.equal(fallback.source, "bundled");

  assert.throws(() => WOLSync.resolveGoogleCredentials(null, undefined), /Google Drive client/);
});
