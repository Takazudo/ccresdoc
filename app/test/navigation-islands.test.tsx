/** @jsxRuntime automatic */
/** @jsxImportSource preact */

import { render } from "preact";
import { act } from "preact/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SidebarTree } from "@takazudo/zudo-doc/sidebar-tree-island";
import { SidebarToggle } from "@takazudo/zudo-doc/sidebar-toggle-island";
import {
  DesktopSidebarToggle,
  SIDEBAR_STORAGE_KEY,
} from "@takazudo/zudo-doc/desktop-sidebar-toggle-island";
import { SiteTreeNav } from "@takazudo/zudo-doc/site-tree-nav-island";
import {
  ConnectorLines,
  connectorLeft,
} from "@takazudo/zudo-doc/tree-nav-shared";
import {
  SmartBreak,
  smartBreakToHtml,
} from "@takazudo/zudo-doc/smart-break";
import {
  findActiveSlug,
  normalizePath,
} from "@takazudo/zudo-doc/sidebar-active-slug";
import {
  CURRENT_PATH_DATASET_KEY,
  readCurrentPath,
} from "@takazudo/zudo-doc/current-path";
import { filterTree } from "@takazudo/zudo-doc/sidebar-filter";
import { buildSidebarForSection } from "@takazudo/zudo-doc/sidebar-utils";
import { schemaVersion } from "@takazudo/zudo-doc/site-schema";
import { ThemeToggle } from "@takazudo/zudo-doc/theme-toggle";
import { AFTER_NAVIGATE_EVENT } from "@takazudo/zudo-doc/transitions";
import type { SidebarNavNode } from "@takazudo/zudo-doc/sidebar/types";

const nodes: SidebarNavNode[] = [
  {
    slug: "welcome",
    label: "Welcome",
    position: 1,
    href: "/docs/welcome/",
    hasPage: true,
    children: [],
  },
  {
    slug: "guides",
    label: "Guides",
    position: 2,
    href: "/docs/guides/",
    hasPage: true,
    collapsed: true,
    children: [
      {
        slug: "guides/https-path",
        label: "https://example.com/a/very/long/path",
        position: 1,
        href: "/docs/guides/https-path/",
        hasPage: true,
        children: [],
      },
      {
        slug: "guides/other",
        label: "Other guide",
        position: 2,
        href: "/docs/guides/other/",
        hasPage: true,
        children: [],
      },
    ],
  },
];

const mountedRoots: HTMLDivElement[] = [];

function mount(node: preact.ComponentChildren): HTMLDivElement {
  const root = document.createElement("div");
  document.body.append(root);
  mountedRoots.push(root);
  act(() => render(node, root));
  return root;
}

function click(element: Element): void {
  act(() => {
    element.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

function input(element: HTMLInputElement, value: string): void {
  act(() => {
    element.value = value;
    element.dispatchEvent(new InputEvent("input", { bubbles: true }));
  });
}

afterEach(() => {
  for (const root of mountedRoots.splice(0)) {
    act(() => render(null, root));
    root.remove();
  }
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-sidebar-hidden");
  document.documentElement.removeAttribute("data-zd-current-path");
  document.documentElement.style.colorScheme = "";
  window.history.replaceState({}, "", "/");
});

describe("published zudo-doc navigation contracts", () => {
  it("uses the browser-safe schema and public navigation helpers", () => {
    expect(schemaVersion).toBe(1);
    expect(normalizePath("/docs/welcome/")).toBe("/docs/welcome");
    expect(findActiveSlug(nodes, "/docs/welcome")).toBe("welcome");
    expect(filterTree(nodes, "other")).toEqual([
      expect.objectContaining({
        slug: "guides",
        children: [expect.objectContaining({ slug: "guides/other" })],
      }),
    ]);

    document.documentElement.dataset[CURRENT_PATH_DATASET_KEY] = "/docs/guides/";
    expect(readCurrentPath(CURRENT_PATH_DATASET_KEY)).toBe("/docs/guides/");

    const explicit = buildSidebarForSection(
      [],
      "en",
      "guides",
      undefined,
      { guides: [{ type: "link", label: "External", href: "https://example.com" }] },
      () => nodes,
      ["guides"],
    );
    expect(explicit).toEqual([
      expect.objectContaining({ label: "External", href: "https://example.com" }),
    ]);
  });

  it("keeps native controls keyboard-focusable and exposes disclosure state", () => {
    const root = mount(<SidebarTree nodes={nodes} currentSlug="welcome" />);
    const active = root.querySelector<HTMLAnchorElement>('a[href="/docs/welcome/"]');
    const disclosure = root.querySelector<HTMLButtonElement>('button[aria-label="Expand Guides"]');

    expect(active?.getAttribute("aria-current")).toBe("page");
    expect(active?.tabIndex).toBe(0);
    expect(disclosure?.getAttribute("aria-expanded")).toBe("false");
    expect(disclosure?.hasAttribute("aria-controls")).toBe(false);
    expect(disclosure?.tabIndex).toBe(0);
    active?.focus();
    expect(document.activeElement).toBe(active);
    disclosure?.focus();
    expect(document.activeElement).toBe(disclosure);

    // zudo-doc 5.6 deliberately publishes a native disclosure/link pattern,
    // not a WAI-ARIA tree with roving tabindex or controlled-region ids. Keep
    // these assertions explicit so a future upstream semantic change gets
    // reviewed rather than hidden.
    expect(root.querySelector('[role="tree"]')).toBeNull();
    expect(root.querySelector('[role="treeitem"]')).toBeNull();
  });

  it("filters descendants, persists open categories, and updates the active path after soft navigation", () => {
    window.history.replaceState({}, "", "/docs/welcome/");
    const root = mount(<SidebarTree nodes={nodes} currentSlug="welcome" />);
    const filter = root.querySelector<HTMLInputElement>('input[aria-label="Filter navigation"]')!;

    input(filter, "very/long");
    expect(root.textContent).toContain("https://example.com/a/very/long/path");
    expect(root.textContent).not.toContain("Other guide");
    expect(root.textContent).not.toContain("Welcome");
    input(filter, "");

    const disclosure = root.querySelector<HTMLButtonElement>('button[aria-label="Expand Guides"]')!;
    click(disclosure);
    expect(disclosure.getAttribute("aria-expanded")).toBe("true");
    expect(JSON.parse(sessionStorage.getItem("zd-sidebar-open") ?? "[]")).toContain("guides");

    window.history.replaceState({}, "", "/docs/guides/other/");
    act(() => {
      document.dispatchEvent(new Event(AFTER_NAVIGATE_EVENT));
    });
    expect(root.querySelector('a[href="/docs/guides/other/"]')?.getAttribute("aria-current")).toBe("page");

    act(() => render(null, root));
    act(() => render(<SidebarTree nodes={nodes} currentSlug="welcome" />, root));
    expect(root.querySelector('button[aria-label="Collapse Guides"]')?.getAttribute("aria-expanded")).toBe("true");
  });

  it("installs and removes one soft-navigation listener per sidebar mount", () => {
    const add = vi.spyOn(document, "addEventListener");
    const remove = vi.spyOn(document, "removeEventListener");
    const root = mount(<SidebarTree nodes={nodes} currentSlug="welcome" />);

    expect(add.mock.calls.filter(([type]) => type === AFTER_NAVIGATE_EVENT)).toHaveLength(1);
    act(() => render(null, root));
    expect(remove.mock.calls.filter(([type]) => type === AFTER_NAVIGATE_EVENT)).toHaveLength(1);
  });
});

describe("published mobile, desktop, theme, and tree presentation islands", () => {
  it("makes the closed mobile drawer inert, locks scroll while open, and closes after navigation", () => {
    const root = mount(<SidebarToggle nodes={nodes} currentSlug="welcome" />);
    const toggle = root.querySelector<HTMLButtonElement>('button[aria-label="Open sidebar"]')!;
    const drawer = root.querySelector<HTMLElement>("[data-zd-mobile-sidebar]")!;

    expect(drawer.hasAttribute("inert")).toBe(true);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    click(toggle);
    expect(drawer.hasAttribute("inert")).toBe(false);
    expect(document.body.style.overflow).toBe("hidden");
    expect(root.querySelector('button[aria-label="Close sidebar"]')?.getAttribute("aria-expanded")).toBe("true");

    act(() => {
      document.dispatchEvent(new Event(AFTER_NAVIGATE_EVENT));
    });
    expect(drawer.hasAttribute("inert")).toBe(true);
    expect(document.body.style.overflow).toBe("");
  });

  it("persists the desktop sidebar toggle and restores its document state after navigation", () => {
    const root = mount(<DesktopSidebarToggle />);
    const toggle = root.querySelector<HTMLButtonElement>('button[aria-label="Hide sidebar"]')!;
    click(toggle);

    expect(localStorage.getItem(SIDEBAR_STORAGE_KEY)).toBe("false");
    expect(document.documentElement.hasAttribute("data-sidebar-hidden")).toBe(true);

    document.documentElement.removeAttribute("data-sidebar-hidden");
    act(() => {
      document.dispatchEvent(new Event(AFTER_NAVIGATE_EVENT));
    });
    expect(document.documentElement.hasAttribute("data-sidebar-hidden")).toBe(true);
  });

  it("switches and persists the package theme through its public island", () => {
    document.documentElement.setAttribute("data-theme", "dark");
    const root = mount(<ThemeToggle defaultMode="dark" />);
    const toggle = root.querySelector<HTMLButtonElement>('button[aria-label="Switch to light mode"]')!;
    click(toggle);

    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(document.documentElement.style.colorScheme).toBe("light");
    expect(localStorage.getItem("zudo-doc-theme")).toBe("light");
    expect(root.querySelector('button[aria-label="Switch to dark mode"]')).not.toBeNull();
  });

  it("renders site-nav disclosures, smart path breaks, and connector geometry from public modules", () => {
    const root = mount(
      <>
        <SiteTreeNav tree={nodes} initiallyCollapsedCategorySlugs={["guides"]} />
        <SmartBreak>https://example.com/a/very/long/path</SmartBreak>
        <div data-connector-fixture>
          <ConnectorLines depth={2} isLast topPad="1rem" />
        </div>
      </>,
    );

    const siteNav = root.querySelector<HTMLElement>("[data-site-nav]")!;
    const disclosure = siteNav.querySelector<HTMLButtonElement>('button[aria-label="Expand Guides"]')!;
    expect(siteNav.getAttribute("aria-label")).toBe("Site index");
    expect(disclosure.getAttribute("aria-expanded")).toBe("false");
    click(disclosure);
    expect(siteNav.textContent).toContain("Other guide");
    expect(root.querySelectorAll("wbr").length).toBeGreaterThan(3);
    expect(smartBreakToHtml("<script>/a/b")).toContain("&lt;script&gt;");
    expect(connectorLeft(2)).toContain("calc(2 * clamp(");
    const connector = root.querySelector("[data-connector-fixture]")!;
    expect(connector.querySelectorAll(":scope > div")).toHaveLength(2);
    expect(connector.querySelector(".border-l")).not.toBeNull();
    expect(connector.querySelector(".border-t")).not.toBeNull();
  });
});
