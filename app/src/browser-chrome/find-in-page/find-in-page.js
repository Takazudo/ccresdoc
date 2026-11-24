const MATCH_CLASS = "find-match";
const ACTIVE_CLASS = "find-match-active";
const MATCH_ATTR = "data-find-match";
const ACTIVE_ATTR = "data-find-active";
const EMPTY_RESULT = Object.freeze({ matches: 0, activeMatchOrdinal: 0 });
const EXCLUDED_SELECTOR = [
  "script",
  "style",
  "noscript",
  "template",
  "button",
  "input",
  "select",
  "textarea",
  "option",
  "[hidden]",
  "[inert]",
  "[aria-hidden='true']",
  "dialog:not([open])",
  "details:not([open]) > :not(summary)",
  "[contenteditable]:not([contenteditable='false'])",
  "[role='button']",
  "[role='checkbox']",
  "[role='combobox']",
  "[role='dialog']",
  "[role='listbox']",
  "[role='menu']",
  "[role='option']",
  "[role='radio']",
  "[role='searchbox']",
  "[role='slider']",
  "[role='spinbutton']",
  "[role='switch']",
  "[role='tab']",
  "[role='textbox']",
  "[data-find-in-page-bar]",
  "[data-ccresdoc-browser-toolbar-shell]"
].join(",");
function isHiddenByStyle(element) {
  const style = getComputedStyle(element);
  return style.display === "none" || style.visibility === "hidden" || style.contentVisibility === "hidden";
}
function isVisibleContent(node, container) {
  const parent = node.parentElement;
  if (!parent || !node.isConnected || !container.contains(node)) return false;
  if (parent.closest(EXCLUDED_SELECTOR)) return false;
  for (let element = parent; element && element !== container; element = element.parentElement) {
    if (isHiddenByStyle(element)) return false;
  }
  return !isHiddenByStyle(container);
}
function createFindInPage() {
  let matchElements = [];
  let activeIndex = -1;
  function clearMarks() {
    const parentsToNormalize = /* @__PURE__ */ new Set();
    for (let i = matchElements.length - 1; i >= 0; i--) {
      const mark = matchElements[i];
      const parent = mark.parentNode;
      if (parent) {
        const textNode = document.createTextNode(mark.textContent || "");
        parent.replaceChild(textNode, mark);
        parentsToNormalize.add(parent);
      }
    }
    for (const parent of parentsToNormalize) {
      parent.normalize();
    }
    matchElements = [];
    activeIndex = -1;
  }
  function setActive(index) {
    if (activeIndex >= 0 && activeIndex < matchElements.length) {
      const prev2 = matchElements[activeIndex];
      prev2.classList.remove(ACTIVE_CLASS);
      prev2.removeAttribute(ACTIVE_ATTR);
    }
    activeIndex = index;
    if (activeIndex >= 0 && activeIndex < matchElements.length) {
      const current = matchElements[activeIndex];
      if (!current.isConnected) {
        clearMarks();
        return;
      }
      current.classList.add(ACTIVE_CLASS);
      current.setAttribute(ACTIVE_ATTR, "true");
      current.scrollIntoView?.({ block: "center", behavior: "auto" });
    }
  }
  function currentResult() {
    if (matchElements.length === 0) {
      return EMPTY_RESULT;
    }
    return {
      matches: matchElements.length,
      activeMatchOrdinal: activeIndex + 1
    };
  }
  function find(container, query) {
    clearMarks();
    if (!query) {
      return EMPTY_RESULT;
    }
    const lowerQuery = query.toLowerCase();
    const walker = document.createTreeWalker(
      container,
      NodeFilter.SHOW_TEXT,
      { acceptNode: (candidate) => isVisibleContent(candidate, container) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT }
    );
    const textNodes = [];
    let node;
    while (node = walker.nextNode()) {
      textNodes.push(node);
    }
    for (const textNode of textNodes) {
      const text = textNode.textContent || "";
      const lowerText = text.toLowerCase();
      const positions = [];
      let searchFrom = 0;
      while (searchFrom < lowerText.length) {
        const idx = lowerText.indexOf(lowerQuery, searchFrom);
        if (idx === -1) break;
        positions.push(idx);
        searchFrom = idx + lowerQuery.length;
      }
      if (positions.length === 0) continue;
      const parent = textNode.parentNode;
      if (!parent || !textNode.isConnected || !container.contains(textNode)) continue;
      let remainingNode = textNode;
      const nodeMarks = [];
      for (let i = positions.length - 1; i >= 0; i--) {
        const pos = positions[i];
        const matchLen = query.length;
        if (pos + matchLen < remainingNode.length) {
          remainingNode.splitText(pos + matchLen);
        }
        let matchNode;
        if (pos > 0) {
          matchNode = remainingNode.splitText(pos);
        } else {
          matchNode = remainingNode;
        }
        const mark = document.createElement("mark");
        mark.className = MATCH_CLASS;
        mark.setAttribute(MATCH_ATTR, "true");
        mark.textContent = matchNode.textContent;
        parent.replaceChild(mark, matchNode);
        nodeMarks.unshift(mark);
      }
      matchElements.push(...nodeMarks);
    }
    if (matchElements.length === 0) {
      return EMPTY_RESULT;
    }
    setActive(0);
    return currentResult();
  }
  function next() {
    if (matchElements.length === 0) {
      return EMPTY_RESULT;
    }
    const newIndex = (activeIndex + 1) % matchElements.length;
    setActive(newIndex);
    return currentResult();
  }
  function prev() {
    if (matchElements.length === 0) {
      return EMPTY_RESULT;
    }
    const newIndex = (activeIndex - 1 + matchElements.length) % matchElements.length;
    setActive(newIndex);
    return currentResult();
  }
  function stop() {
    clearMarks();
  }
  return { find, next, prev, stop };
}
export {
  createFindInPage
};
