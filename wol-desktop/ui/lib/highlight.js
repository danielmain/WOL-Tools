const SKIP_ANCESTOR = ".parNum, .pageNum, .fn, rt, rp";

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

function wrapSlice(node, start, end, color, startToken, endToken) {
  let target = node;
  if (end < target.nodeValue.length) target.splitText(end);
  const marked = start > 0 ? target.splitText(start) : target;
  const mark = marked.ownerDocument.createElement("mark");
  const safeColor = color >= 1 && color <= 6 ? color : 0;
  mark.className = `wol-hl wol-c${safeColor}`;
  if (startToken !== undefined) mark.dataset.startToken = String(startToken);
  if (endToken !== undefined) mark.dataset.endToken = String(endToken);
  mark.dataset.color = String(safeColor);
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
    slices.push(...pieces.map((piece) => ({ ...piece, color, startToken, endToken })));
  }
  slices.sort(documentOrderDescending);
  for (const piece of slices) wrapSlice(piece.node, piece.start, piece.end, piece.color, piece.startToken, piece.endToken);
  return { applied, skipped };
}

function getCharOffsetInParts(parts, container, offset) {
  if (!parts || !parts.length) return 0;
  if (container.nodeType === Node.TEXT_NODE) {
    for (const part of parts) {
      if (part.node === container) {
        return part.stringStart + Math.min(Math.max(0, offset), part.node.nodeValue.length);
      }
    }
  }
  if (container.nodeType === Node.ELEMENT_NODE) {
    if (offset >= 0 && offset < container.childNodes.length) {
      const child = container.childNodes[offset];
      for (const part of parts) {
        if (child === part.node || child.contains(part.node)) {
          return part.stringStart;
        }
        if (child.compareDocumentPosition(part.node) & Node.DOCUMENT_POSITION_FOLLOWING) {
          return part.stringStart;
        }
      }
    } else if (offset >= container.childNodes.length && container.childNodes.length > 0) {
      const lastChild = container.childNodes[container.childNodes.length - 1];
      for (let i = parts.length - 1; i >= 0; i--) {
        const part = parts[i];
        if (lastChild === part.node || lastChild.contains(part.node)) {
          return part.stringEnd;
        }
        if (lastChild.compareDocumentPosition(part.node) & Node.DOCUMENT_POSITION_PRECEDING) {
          return part.stringEnd;
        }
      }
    }
  }
  for (const part of parts) {
    if (container.compareDocumentPosition(part.node) & Node.DOCUMENT_POSITION_FOLLOWING) {
      return part.stringStart;
    }
  }
  return parts[parts.length - 1].stringEnd;
}

function rangeIntersectsNode(range, node) {
  if (!range || !node) return false;
  if (typeof range.intersectsNode === "function") {
    try {
      return range.intersectsNode(node);
    } catch {
      // Fall through to fallback
    }
  }
  return (
    node.contains(range.startContainer) ||
    node.contains(range.endContainer) ||
    range.commonAncestorContainer === node ||
    Boolean(range.commonAncestorContainer && node.contains(range.commonAncestorContainer))
  );
}

function tokenRangeForBlock(roots, domRange) {
  const list = (Array.isArray(roots) ? roots : [roots]).filter(Boolean);
  if (!list.length || !domRange || domRange.collapsed) return null;

  const collected = collectText(list);
  if (!collected.text || !collected.parts.length) return null;

  const tokens = tokenize(collected.text);
  if (!tokens.length) return null;

  let charStart = 0;
  let charEnd = collected.text.length;

  const firstRoot = list[0];
  const lastRoot = list[list.length - 1];

  const startsInside = list.some((r) => r.contains(domRange.startContainer));
  if (startsInside) {
    charStart = getCharOffsetInParts(collected.parts, domRange.startContainer, domRange.startOffset);
  } else {
    const pos = firstRoot.compareDocumentPosition(domRange.startContainer);
    if (pos & Node.DOCUMENT_POSITION_FOLLOWING) return null;
    charStart = 0;
  }

  const endsInside = list.some((r) => r.contains(domRange.endContainer));
  if (endsInside) {
    charEnd = getCharOffsetInParts(collected.parts, domRange.endContainer, domRange.endOffset);
  } else {
    const pos = lastRoot.compareDocumentPosition(domRange.endContainer);
    if (pos & Node.DOCUMENT_POSITION_PRECEDING) return null;
    charEnd = collected.text.length;
  }

  if (charStart >= charEnd) return null;

  let startToken = -1;
  let endToken = -1;

  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i];
    if (Math.max(t.start, charStart) < Math.min(t.end, charEnd)) {
      if (startToken === -1) {
        startToken = i;
      }
      endToken = i;
    }
  }

  if (startToken === -1 || endToken === -1 || startToken > endToken) {
    return null;
  }

  return { startToken, endToken };
}

function addHighlightRange(ranges, [newStart, newEnd, newColor]) {
  const result = [];
  let s = newStart;
  let e = newEnd;
  for (const [start, end, color] of ranges) {
    if (end < s || start > e) {
      result.push([start, end, color]);
      continue;
    }
    if (color === newColor) {
      s = Math.min(s, start);
      e = Math.max(e, end);
      continue;
    }
    if (start < s) {
      result.push([start, s - 1, color]);
    }
    if (end > e) {
      result.push([e + 1, end, color]);
    }
  }
  result.push([s, e, newColor]);
  result.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  return result;
}

function removeHighlightRange(ranges, [remStart, remEnd]) {
  const result = [];
  for (const [start, end, color] of ranges) {
    if (end < remStart || start > remEnd) {
      result.push([start, end, color]);
      continue;
    }
    if (start < remStart) {
      result.push([start, remStart - 1, color]);
    }
    if (end > remEnd) {
      result.push([remEnd + 1, end, color]);
    }
  }
  result.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  return result;
}
