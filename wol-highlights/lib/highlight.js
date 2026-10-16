const SKIP_ANCESTOR = ".parNum, .pageNum, .fn, rt, rp, mark.wol-hl";

function acceptText(node) {
  if (!node.nodeValue) return NodeFilter.FILTER_REJECT;
  const parent = node.parentElement;
  if (!parent) return NodeFilter.FILTER_REJECT;
  if (parent.closest(SKIP_ANCESTOR)) return NodeFilter.FILTER_REJECT;
  if (parent.closest("a.vl, a.cl")) return NodeFilter.FILTER_REJECT;
  const reference = parent.closest("a.b");
  if (reference && reference.textContent.trim() === "+") return NodeFilter.FILTER_REJECT;
  return NodeFilter.FILTER_ACCEPT;
}

function collectText(roots) {
  const list = Array.isArray(roots) ? roots : [roots];
  const documentRef = list[0]?.ownerDocument;
  if (!documentRef) return { text: "", parts: [] };
  const parts = [];
  let text = "";
  for (const root of list) {
    if (!root) continue;
    const walker = documentRef.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode: acceptText,
    });
    let node = walker.nextNode();
    while (node) {
      const value = node.nodeValue;
      parts.push({ node, stringStart: text.length, stringEnd: text.length + value.length });
      text += value;
      node = walker.nextNode();
    }
  }
  return { text, parts };
}

function clearMarks(root) {
  if (!root) return;
  const marks = [...root.querySelectorAll("mark.wol-hl")];
  const parents = new Set();
  for (const mark of marks) {
    const parent = mark.parentNode;
    if (!parent) continue;
    while (mark.firstChild) parent.insertBefore(mark.firstChild, mark);
    parent.removeChild(mark);
    parents.add(parent);
  }
  for (const parent of parents) parent.normalize();
}

function slicesFor(tokens, parts, startToken, endToken) {
  if (startToken < 0 || endToken < startToken || endToken >= tokens.length) return null;
  const from = tokens[startToken].start;
  const to = tokens[endToken].end;
  const slices = [];
  for (const part of parts) {
    const start = Math.max(part.stringStart, from);
    const end = Math.min(part.stringEnd, to);
    if (start < end) {
      slices.push({
        node: part.node,
        start: start - part.stringStart,
        end: end - part.stringStart,
      });
    }
  }
  return slices;
}

function documentOrderDescending(left, right) {
  if (left.node === right.node) return right.start - left.start;
  const position = left.node.compareDocumentPosition(right.node);
  if (position & Node.DOCUMENT_POSITION_FOLLOWING) return 1;
  if (position & Node.DOCUMENT_POSITION_PRECEDING) return -1;
  return 0;
}

function wrapSlice(node, start, end, color) {
  let target = node;
  if (end < target.nodeValue.length) target.splitText(end);
  const marked = start > 0 ? target.splitText(start) : target;
  const mark = marked.ownerDocument.createElement("mark");
  const safeColor = color >= 1 && color <= 6 ? color : 0;
  mark.className = `wol-hl wol-c${safeColor}`;
  marked.parentNode.insertBefore(mark, marked);
  mark.appendChild(marked);
}

// ranges are [startToken, endToken, colorIndex], endToken inclusive.
// roots is one element, or several elements that form one verse in order.
function highlightBlock(roots, ranges) {
  const list = (Array.isArray(roots) ? roots : [roots]).filter(Boolean);
  if (!list.length || !ranges?.length) return { applied: 0, skipped: 0 };
  for (const root of list) clearMarks(root);
  const collected = collectText(list);
  const tokens = tokenize(collected.text);
  const slices = [];
  let applied = 0;
  let skipped = 0;
  for (const [startToken, endToken, color] of ranges) {
    const pieces = slicesFor(tokens, collected.parts, startToken, endToken);
    if (!pieces || !pieces.length) {
      console.warn("[WOL Highlights DOM] Range skipped - token range invalid or empty:", {
        startToken,
        endToken,
        totalTokens: tokens.length,
        textSnippet: collected.text.slice(0, 50),
      });
      skipped += 1;
      continue;
    }
    applied += 1;
    slices.push(...pieces.map((piece) => ({ ...piece, color })));
  }
  slices.sort(documentOrderDescending);
  for (const piece of slices) wrapSlice(piece.node, piece.start, piece.end, piece.color);
  return { applied, skipped };
}
