// JWPUB document content uses AES-CBC followed by zlib compression.
// The publication identity and mask described by jwpub-toolkit derive the key.
const MASK = Uint8Array.from([
  0x11, 0xcb, 0xb5, 0x58, 0x7e, 0x32, 0x84, 0x6d,
  0x4c, 0x26, 0x79, 0x0c, 0x63, 0x3d, 0xa2, 0x89,
  0xf6, 0x6f, 0xe5, 0x84, 0x2a, 0x3a, 0x58, 0x5c,
  0xe1, 0xbc, 0x3a, 0x29, 0x4a, 0xf5, 0xad, 0xa7,
]);

export async function jwpubKeyMaterial({ language, symbol, year, issue = 0 }) {
  if (!Number.isInteger(language) || !Number.isInteger(year) || !Number.isInteger(issue) || !symbol) {
    throw new Error("Invalid publication identity");
  }
  const identity = `${language}_${symbol}_${year}${issue ? `_${issue}` : ""}`;
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(identity)));
  return digest.map((value, index) => value ^ MASK[index]);
}

export async function decodeJwpubContent(content, publication) {
  const material = await jwpubKeyMaterial(publication);
  const key = await crypto.subtle.importKey("raw", material.slice(0, 16), "AES-CBC", false, ["decrypt"]);
  const compressed = await crypto.subtle.decrypt({ name: "AES-CBC", iv: material.slice(16) }, key, content);
  const stream = new Blob([compressed]).stream().pipeThrough(new DecompressionStream("deflate"));
  return new TextDecoder("utf-8", { fatal: true }).decode(await new Response(stream).arrayBuffer());
}
