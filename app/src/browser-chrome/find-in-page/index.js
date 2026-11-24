"use client";
import { jsx } from "@takazudo/zfb/zudo-react/jsx-runtime";
import { computed, getScope, Show, signal } from "@takazudo/zfb/zudo-react";
import { FindBar } from "./find-bar.js";
import { createFindInPage } from "./find-in-page.js";
const CONTENT_SELECTOR = "article.zd-content";
const BEFORE_PREPARATION_EVENT = "zfb:before-preparation";
const AFTER_SWAP_EVENT = "zfb:after-swap";
const FIND_IN_PAGE_COMMAND_EVENT = "zudo-doc:find-in-page-command";
function dispatchFindCommand(action) {
  document.dispatchEvent(new CustomEvent(FIND_IN_PAGE_COMMAND_EVENT, { detail: { action } }));
}
function openFindInPage() {
  dispatchFindCommand("open");
}
function closeFindInPage() {
  dispatchFindCommand("close");
}
function toggleFindInPage() {
  dispatchFindCommand("toggle");
}
function FindInPageInit(props = {}) {
  const disableBuiltInShortcut = props.disableBuiltInShortcut === true;
  const isTauri = signal(false);
  const visible = signal(false);
  const focusRequest = signal(0);
  const findInPage = createFindInPage();
  const scope = getScope();
  const open = () => {
    visible.value = true;
    focusRequest.value += 1;
  };
  const close = () => {
    findInPage.stop();
    visible.value = false;
  };
  scope.onActivate(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    isTauri.value = true;
    const handleKeyDown = (event) => {
      const keyboard = event;
      if (keyboard.isComposing || event.defaultPrevented || !(keyboard.metaKey || keyboard.ctrlKey) || keyboard.key !== "f") {
        return;
      }
      event.preventDefault();
      if (visible.value) close();
      else open();
    };
    const handleCommand = (event) => {
      const action = event.detail?.action;
      if (action === "open") open();
      else if (action === "close") close();
      else if (action === "toggle") {
        if (visible.value) close();
        else open();
      }
    };
    if (!disableBuiltInShortcut) document.addEventListener("keydown", handleKeyDown);
    document.addEventListener(FIND_IN_PAGE_COMMAND_EVENT, handleCommand);
    document.addEventListener(BEFORE_PREPARATION_EVENT, close);
    document.addEventListener(AFTER_SWAP_EVENT, close);
    return () => {
      if (!disableBuiltInShortcut) document.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener(FIND_IN_PAGE_COMMAND_EVENT, handleCommand);
      document.removeEventListener(BEFORE_PREPARATION_EVENT, close);
      document.removeEventListener(AFTER_SWAP_EVENT, close);
      findInPage.stop();
    };
  });
  return /* @__PURE__ */ jsx(Show, { when: computed(() => isTauri.value), children: () => /* @__PURE__ */ jsx(
    FindBar,
    {
      visible,
      focusRequest,
      onClose: close,
      findInPage,
      containerSelector: CONTENT_SELECTOR
    }
  ) });
}
FindInPageInit.displayName = "FindInPageInit";
export {
  FIND_IN_PAGE_COMMAND_EVENT,
  FindInPageInit,
  closeFindInPage,
  createFindInPage,
  openFindInPage,
  toggleFindInPage
};
