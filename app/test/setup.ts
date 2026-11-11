import { afterEach } from "vitest";
import { unmountAll } from "./helpers/zudo-react";

// Keep DOM state isolated even when a later suite enables Vitest's in-band mode.
afterEach(() => {
  unmountAll();
  document.body.replaceChildren();
  window.localStorage.clear();
  window.sessionStorage.clear();
});

if (!window.matchMedia) {
  window.matchMedia = (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener() {},
    removeListener() {},
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent() { return false; },
  });
}
