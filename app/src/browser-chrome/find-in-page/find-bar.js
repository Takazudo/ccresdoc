import { jsx, jsxs } from "@takazudo/zfb/zudo-react/jsx-runtime";
import {
  computed,
  getScope,
  Show,
  signal
} from "@takazudo/zfb/zudo-react";
function toMatchInfo(result) {
  return result.matches > 0 ? result : null;
}
function FindBar({ visible, focusRequest, onClose, findInPage, containerSelector }) {
  const query = signal("");
  const matchInfo = signal(null);
  const inputRef = { current: null };
  const scope = getScope();
  scope.effect(() => {
    if (!visible.value) {
      query.value = "";
      matchInfo.value = null;
      findInPage.stop();
      return;
    }
    const text = query.value;
    const container = document.querySelector(containerSelector);
    if (!text || !(container instanceof HTMLElement)) {
      matchInfo.value = null;
      findInPage.stop();
      return;
    }
    matchInfo.value = toMatchInfo(findInPage.find(container, text));
  });
  scope.effect(() => {
    if (!visible.value) return;
    void focusRequest?.value;
    // WebKit-only attribute outside the zudo-react JSX vocabulary.
    inputRef.current?.setAttribute("autocorrect", "off");
    inputRef.current?.focus();
    inputRef.current?.select();
  });
  scope.onCleanup(() => findInPage.stop());
  const handleKeyDown = (event) => {
    const keyboard = event;
    if (keyboard.isComposing || event.defaultPrevented) return;
    if (keyboard.key === "Escape") {
      onClose();
    } else if (keyboard.key === "Enter") {
      const result = keyboard.shiftKey ? findInPage.prev() : findInPage.next();
      matchInfo.value = toMatchInfo(result);
    }
  };
  return /* @__PURE__ */ jsx(Show, { when: computed(() => visible.value), children: () => /* @__PURE__ */ jsxs("div", { "data-find-in-page-bar": true, class: "fixed top-[3.5rem] right-0 z-dropdown flex max-w-full items-center gap-hsp-xs py-hsp-xs px-hsp-sm bg-surface border-b border-l border-muted rounded-bl-lg", children: [
    /* @__PURE__ */ jsx(
      "input",
      {
        ref: inputRef,
        class: "min-w-0 w-[12rem] py-[4px] px-hsp-sm rounded text-small bg-bg border border-muted text-fg outline-none focus:border-accent",
        type: "search",
        autocomplete: "off",
        autocapitalize: "off",
        spellcheck: "false",
        modelValue: query,
        placeholder: "Find in page...",
        "aria-label": "Find in page",
        "on:keydown": handleKeyDown
      }
    ),
    /* @__PURE__ */ jsx("span", { class: "text-caption whitespace-nowrap min-w-[3rem] text-center text-fg/60", "aria-live": "polite", children: computed(() => {
      const result = matchInfo.value;
      return result ? `${result.activeMatchOrdinal}/${result.matches}` : "";
    }) }),
    /* @__PURE__ */ jsx(
      "button",
      {
        type: "button",
        class: "find-in-page-control",
        "on:click": () => {
          matchInfo.value = toMatchInfo(findInPage.prev());
        },
        title: "Previous (Shift+Enter)",
        "aria-label": "Previous match (Shift+Enter)",
        children: /* @__PURE__ */ jsx("svg", { viewBox: "0 0 24 24", width: "18", height: "18", fill: "none", stroke: "currentColor", "stroke-width": "2", "stroke-linecap": "round", "stroke-linejoin": "round", "aria-hidden": "true", children: /* @__PURE__ */ jsx("path", { d: "m18 15-6-6-6 6" }) })
      }
    ),
    /* @__PURE__ */ jsx(
      "button",
      {
        type: "button",
        class: "find-in-page-control",
        "on:click": () => {
          matchInfo.value = toMatchInfo(findInPage.next());
        },
        title: "Next (Enter)",
        "aria-label": "Next match (Enter)",
        children: /* @__PURE__ */ jsx("svg", { viewBox: "0 0 24 24", width: "18", height: "18", fill: "none", stroke: "currentColor", "stroke-width": "2", "stroke-linecap": "round", "stroke-linejoin": "round", "aria-hidden": "true", children: /* @__PURE__ */ jsx("path", { d: "m6 9 6 6 6-6" }) })
      }
    ),
    /* @__PURE__ */ jsx(
      "button",
      {
        type: "button",
        class: "find-in-page-control",
        "on:click": onClose,
        title: "Close (Esc)",
        "aria-label": "Close find in page (Escape)",
        children: /* @__PURE__ */ jsx("svg", { viewBox: "0 0 24 24", width: "18", height: "18", fill: "none", stroke: "currentColor", "stroke-width": "2", "stroke-linecap": "round", "stroke-linejoin": "round", "aria-hidden": "true", children: /* @__PURE__ */ jsx("path", { d: "M18 6 6 18M6 6l12 12" }) })
      }
    )
  ] }) });
}
export {
  FindBar
};
