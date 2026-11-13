// WOL puts the MEPS written-language symbol in the URL (`lp-s`) and on
// the article (`data-lang="S"`). userData.db stores the numeric id.
// Document highlights do not need this map: a document id is unique.
// Bible chapters do. English 0 and Spanish 1 are the ids JW Library uses.
// Any other language still matches when this backup has that chapter in
// only one language.

const MEPS_BY_SYMBOL = {
  E: 0,
  S: 1,
  F: 2,
  X: 3,
  I: 4,
  T: 5,
  J: 6,
  O: 7,
  Z: 8,
  D: 9,
  FI: 10,
  N: 11,
  G: 12,
  P: 13,
  H: 14,
  U: 15,
  B: 16,
  TG: 17,
  CV: 18,
  IL: 19,
};

function languageId(symbol) {
  if (!symbol) return null;
  const id = MEPS_BY_SYMBOL[String(symbol).toUpperCase()];
  return id === undefined ? null : id;
}
