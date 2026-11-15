import { test } from "node:test";
import assert from "node:assert/strict";
import { webcrypto } from "node:crypto";
import { deflateSync } from "node:zlib";
import { jwpubKeyMaterial, decodeJwpubContent } from "../../wol-desktop/ui/lib/jwpub-crypto.mjs";

globalThis.crypto ??= webcrypto;

test("decodes encrypted publication content and rejects the wrong identity", async () => {
  const publication = { language: 1, symbol: "example", year: 2025 };
  const html = '<p data-pid="24">Texto de prueba.</p>';
  const material = await jwpubKeyMaterial(publication);
  const key = await crypto.subtle.importKey("raw", material.slice(0, 16), "AES-CBC", false, ["encrypt"]);
  const blob = await crypto.subtle.encrypt({ name: "AES-CBC", iv: material.slice(16) }, key, deflateSync(html));
  assert.equal(await decodeJwpubContent(blob, publication), html);
  await assert.rejects(decodeJwpubContent(blob, { ...publication, year: 2024 }));
});
