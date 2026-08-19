import { afterEach, describe, expect, it, vi } from "vitest";
import {
  initSidebarResizer,
  SIDEBAR_RESIZER_INIT_SCRIPT,
  SIDEBAR_RESIZER_RESTORE_SCRIPT,
} from "@takazudo/zudo-doc/sidebar-resizer";

const HANDLE_SELECTOR = "[data-sidebar-resizer]";

function pointerEvent(
  type: string,
  { clientX, pointerId = 1 }: { clientX: number; pointerId?: number },
): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperties(event, {
    clientX: { value: clientX },
    pointerId: { value: pointerId },
  });
  return event;
}

function createSidebar(width = 320): {
  sidebar: HTMLElement;
  handle: HTMLElement;
} {
  document.documentElement.style.setProperty("--zd-sidebar-w", `${width}px`);
  document.documentElement.style.setProperty("--zd-accent", "rgb(0 100 200)");
  const sidebar = document.createElement("aside");
  sidebar.id = "desktop-sidebar";
  sidebar.style.position = "fixed";
  vi.spyOn(sidebar, "getBoundingClientRect").mockReturnValue({
    x: 0,
    y: 56,
    top: 56,
    right: width,
    bottom: 800,
    left: 0,
    width,
    height: 744,
    toJSON: () => ({}),
  });
  document.body.append(sidebar);
  initSidebarResizer();
  const handle = sidebar.querySelector<HTMLElement>(HANDLE_SELECTOR);
  expect(handle).not.toBeNull();
  Object.defineProperty(handle!, "setPointerCapture", {
    configurable: true,
    value: vi.fn(),
  });
  return { sidebar, handle: handle! };
}

afterEach(() => {
  document.documentElement.style.removeProperty("--zd-sidebar-w");
  document.documentElement.style.removeProperty("--zd-accent");
  document.documentElement.style.cursor = "";
  document.documentElement.style.userSelect = "";
});

describe("published sidebar resizer consumer contract", () => {
  it("exposes a computed 16px straddling hitbox and separator semantics", () => {
    const { sidebar, handle } = createSidebar();
    const style = getComputedStyle(handle);

    expect(parseFloat(style.width)).toBeGreaterThanOrEqual(16);
    // happy-dom drops calc(var()) from CSSStyleDeclaration, so pin the
    // package's emitted geometry while still measuring the rendered hitbox.
    expect(SIDEBAR_RESIZER_INIT_SCRIPT).toContain(
      'left:"calc(var(--zd-sidebar-w) - 4px)",width:"16px"',
    );
    expect(handle.style.right).toBe("");
    expect(handle.style.bottom).toBe("0px");
    expect(sidebar.getBoundingClientRect().right).toBe(320);
    expect(handle).toMatchObject({ tabIndex: 0 });
    expect(handle.getAttribute("role")).toBe("separator");
    expect(handle.getAttribute("aria-orientation")).toBe("vertical");
    expect(handle.getAttribute("aria-label")).toBe("Resize sidebar");
    expect(handle.getAttribute("aria-valuemin")).toBe("192");
    expect(handle.getAttribute("aria-valuemax")).toBe("448");
    expect(handle.getAttribute("aria-valuenow")).toBe("320");
  });

  it("provides hover and keyboard-focus feedback through semantic tokens", () => {
    const { handle } = createSidebar();

    handle.focus();
    expect(document.activeElement).toBe(handle);
    handle.blur();
    // happy-dom also rejects var()-backed inline colors; assert the emitted
    // handlers and semantic-token values rather than replacing package logic.
    expect(SIDEBAR_RESIZER_INIT_SCRIPT).toContain(
      'ACCENT_BG="var(--zd-accent,rgba(128,128,128,0.3))"',
    );
    expect(SIDEBAR_RESIZER_INIT_SCRIPT).toContain(
      'ACCENT_OUTLINE="2px solid var(--zd-accent,rgba(128,128,128,0.5))"',
    );
    expect(SIDEBAR_RESIZER_INIT_SCRIPT).toContain(
      'handle.addEventListener("mouseenter"',
    );
    expect(SIDEBAR_RESIZER_INIT_SCRIPT).toContain(
      'handle.addEventListener("focus"',
    );
  });

  it("supports Arrow/Home/End keys and clamps persisted width to 192–448px", () => {
    const { handle } = createSidebar();

    handle.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight" }));
    expect(document.documentElement.style.getPropertyValue("--zd-sidebar-w"))
      .toBe("330px");
    expect(localStorage.getItem("zudo-doc-sidebar-width")).toBe("330");
    expect(handle.getAttribute("aria-valuenow")).toBe("330");

    handle.dispatchEvent(new KeyboardEvent("keydown", { key: "Home" }));
    expect(document.documentElement.style.getPropertyValue("--zd-sidebar-w"))
      .toBe("192px");
    handle.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft" }));
    expect(handle.getAttribute("aria-valuenow")).toBe("192");

    handle.dispatchEvent(new KeyboardEvent("keydown", { key: "End" }));
    expect(document.documentElement.style.getPropertyValue("--zd-sidebar-w"))
      .toBe("448px");
    handle.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight" }));
    expect(handle.getAttribute("aria-valuenow")).toBe("448");
  });

  it("drags without edge snapping, commits, and restores localStorage", () => {
    const { handle } = createSidebar();

    handle.dispatchEvent(pointerEvent("pointerdown", { clientX: 326 }));
    expect(document.documentElement.style.cursor).toBe("col-resize");
    expect(document.documentElement.style.userSelect).toBe("none");
    expect(document.body.querySelector('[style*="height: 100vh"]')).not.toBeNull();

    handle.dispatchEvent(pointerEvent("pointermove", { clientX: 406 }));
    handle.dispatchEvent(pointerEvent("pointerup", { clientX: 406 }));
    expect(document.documentElement.style.getPropertyValue("--zd-sidebar-w"))
      .toBe("400px");
    expect(localStorage.getItem("zudo-doc-sidebar-width")).toBe("400");
    expect(handle.getAttribute("aria-valuenow")).toBe("400");
    expect(document.documentElement.style.cursor).toBe("");
    expect(document.documentElement.style.userSelect).toBe("");

    document.documentElement.style.removeProperty("--zd-sidebar-w");
    window.eval(SIDEBAR_RESIZER_RESTORE_SCRIPT);
    expect(document.documentElement.style.getPropertyValue("--zd-sidebar-w"))
      .toBe("400px");
  });

  it("clamps restored out-of-range values and stays idempotent after navigation", () => {
    localStorage.setItem("zudo-doc-sidebar-width", "999");
    window.eval(SIDEBAR_RESIZER_RESTORE_SCRIPT);
    expect(document.documentElement.style.getPropertyValue("--zd-sidebar-w"))
      .toBe("448px");

    const { sidebar } = createSidebar(448);
    initSidebarResizer();
    document.dispatchEvent(new Event("zfb:after-swap"));
    expect(sidebar.querySelectorAll(HANDLE_SELECTOR)).toHaveLength(1);
  });
});
