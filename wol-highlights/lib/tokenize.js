// JW Library counts a token as a word or a single punctuation mark.
// Whitespace, including non-breaking spaces, is not a token.
// This matches Python 3's Unicode \w (letters, numbers, connector
// punctuation such as _) and treats every other non-space character
// as its own token. Verified against this backup on wol.jw.org:
// document 2025441 paragraph 7, and nwtsty Exodus 5:9 and 6:6.

const WORD = /[\p{L}\p{N}\p{Pc}]/u;
const SPACE = /[\s\u00A0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000\uFEFF]/u;

function tokenize(text) {
  const tokens = [];
  const length = text.length;
  let index = 0;
  while (index < length) {
    const character = text[index];
    if (SPACE.test(character)) {
      index += 1;
      continue;
    }
    if (WORD.test(character)) {
      const start = index;
      index += 1;
      while (index < length && WORD.test(text[index])) index += 1;
      tokens.push({ start, end: index });
      continue;
    }
    tokens.push({ start: index, end: index + 1 });
    index += 1;
  }
  return tokens;
}
