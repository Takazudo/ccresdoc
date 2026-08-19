/** @jsxRuntime automatic */
/** @jsxImportSource preact */

import { render } from "preact";
import { act } from "preact/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import themePackCatalog from "@takazudo/zudo-doc/catalog";
import { ThemeToggle } from "@takazudo/zudo-doc/theme";
import { settings, themePackSlugs } from "@/config/settings";
import { themePackRegistry } from "../pages/lib/_route-context";
import { config } from "../zfb.config";
import { ThemePackSwitcher } from "../node_modules/@takazudo/zudo-doc/dist/theme-pack-switcher/index.js";
import {
  applyThemePack,
  THEME_PACK_ATTR,
  THEME_PACK_CHANGED_EVENT,
  THEME_PACK_LINK_ATTR,
  THEME_PACK_LINK_LOADING_ATTR,
  THEME_PACK_RUNTIME_GLOBAL,
  THEME_PACK_STORAGE_KEY,
} from "../node_modules/@takazudo/zudo-doc/dist/theme-pack-switcher/theme-pack-sync.js";
import {
  buildThemePackBootstrap,
  THEME_PACK_LOADING_ATTR,
} from "../node_modules/@takazudo/zudo-doc/dist/theme/theme-pack-provider.js";

type RuntimeWindow = Window &
  typeof globalThis & {
    [THEME_PACK_RUNTIME_GLOBAL]?: {
      base: string;
      packs: Record<string, string>;
      configured: string;
    };
  };

const runtimeWindow = window as RuntimeWindow;
const versions = Object.fromEntries(
  themePackCatalog.packs.map(({ slug, version }) => [slug, version]),
);

function setRuntime(base = "data:text/css,"): void {
  runtimeWindow[THEME_PACK_RUNTIME_GLOBAL] = {
    base,
    packs: versions,
    configured: "default",
  };
}

function click(element: Element): void {
  act(() => {
    element.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

async function settlePackLink(event: "load" | "error") {
  await Promise.resolve();
  const link = document.head.querySelector<HTMLLinkElement>(
    `link[${THEME_PACK_LINK_LOADING_ATTR}]`,
  );
  expect(link).not.toBeNull();
  link!.dispatchEvent(new Event(event));
}

afterEach(() => {
  document.head
    .querySelectorAll(
      `link[${THEME_PACK_LINK_ATTR}],link[${THEME_PACK_LINK_LOADING_ATTR}]`,
    )
    .forEach((link) => link.remove());
  document.documentElement.removeAttribute(THEME_PACK_ATTR);
  document.documentElement.removeAttribute(THEME_PACK_LOADING_ATTR);
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.style.colorScheme = "";
  delete runtimeWindow[THEME_PACK_RUNTIME_GLOBAL];
});

describe("CCResDoc theme-pack host integration", () => {
  it("derives settings and the typed route registry from the public catalog", () => {
    expect(themePackSlugs).toEqual(
      themePackCatalog.packs.map(({ slug }) => slug),
    );
    expect(settings.themePack).toBe("default");
    expect(settings.themePackSwitcher).toBe(true);
    expect(settings.themePacks).toEqual(themePackSlugs);
    expect(themePackRegistry).toEqual(
      themePackCatalog.packs.map((meta) => ({
        slug: meta.slug,
        meta,
        hasStylesheet: meta.slug !== "default",
      })),
    );
    expect(themePackRegistry[0]).toMatchObject({
      slug: "default",
      hasStylesheet: false,
    });
    expect(themePackRegistry.slice(1).every(({ hasStylesheet }) => hasStylesheet))
      .toBe(true);
  });

  it("keeps the compatible preset settings and intentional host deviations", () => {
    expect(settings).toMatchObject({
      cjkFriendly: true,
      imageEnlarge: true,
      tocToggle: true,
      noindex: true,
      sidebarResizer: true,
      sidebarToggle: true,
      dynamicPageTransition: true,
      findInPage: true,
      claudeResources: false,
      packageOwnedRoutes: false,
      headerNav: [],
      metaTags: {
        description: false,
        keywords: false,
        ogImage: false,
        ogSiteName: false,
        twitterCard: false,
      },
    });
    expect(settings.headerRightItems).toEqual([
      { type: "component", component: "theme-toggle" },
    ]);
    expect(config.plugins).toEqual([]);
    expect(config.markdown?.cjkFriendly).toBe(true);
  });

  it("rolls back a failed stylesheet atomically without storage or events", async () => {
    setRuntime();
    document.documentElement.setAttribute(THEME_PACK_ATTR, "foundry");
    const current = document.createElement("link");
    current.rel = "stylesheet";
    current.setAttribute(THEME_PACK_LINK_ATTR, "");
    current.href = "data:text/css,html%7Bcolor:inherit%7D";
    document.head.append(current);
    localStorage.setItem(THEME_PACK_STORAGE_KEY, "foundry");
    const changed = vi.fn();
    window.addEventListener(THEME_PACK_CHANGED_EVENT, changed, { once: true });

    let incoming: HTMLLinkElement | null = null;
    const append = vi
      .spyOn(document.head, "appendChild")
      .mockImplementation((node) => {
        incoming = node as HTMLLinkElement;
        return node;
      });
    const result = applyThemePack("matcha");
    await Promise.resolve();
    expect(incoming).not.toBeNull();
    incoming!.dispatchEvent(new Event("error"));

    await expect(result).resolves.toBe(false);
    append.mockRestore();
    expect(document.documentElement.getAttribute(THEME_PACK_ATTR)).toBe(
      "foundry",
    );
    expect(current.isConnected).toBe(true);
    expect(localStorage.getItem(THEME_PACK_STORAGE_KEY)).toBe("foundry");
    expect(changed).not.toHaveBeenCalled();
    window.removeEventListener(THEME_PACK_CHANGED_EVENT, changed);
  });

  it("replaces font-bearing pack stylesheets and preserves color-mode state", async () => {
    setRuntime();
    document.documentElement.setAttribute(THEME_PACK_ATTR, "default");
    document.documentElement.setAttribute("data-theme", "light");
    localStorage.setItem("zudo-doc-theme", "light");
    const changes: Array<{ pack: string; previous: string }> = [];
    const onChange = (event: Event) => {
      changes.push(
        (event as CustomEvent<{ pack: string; previous: string }>).detail,
      );
    };
    window.addEventListener(THEME_PACK_CHANGED_EVENT, onChange);

    const foundry = applyThemePack("foundry");
    await settlePackLink("load");
    await expect(foundry).resolves.toBe(true);
    const foundryLink = document.head.querySelector<HTMLLinkElement>(
      `link[${THEME_PACK_LINK_ATTR}]`,
    );
    expect(foundryLink?.getAttribute("href")).toContain(
      "theme-packs/foundry/pack.css",
    );

    const matcha = applyThemePack("matcha");
    await settlePackLink("load");
    await expect(matcha).resolves.toBe(true);
    const activeLinks = document.head.querySelectorAll<HTMLLinkElement>(
      `link[${THEME_PACK_LINK_ATTR}]`,
    );
    expect(activeLinks).toHaveLength(1);
    expect(activeLinks[0]?.getAttribute("href")).toContain(
      "theme-packs/matcha/pack.css",
    );
    expect(foundryLink?.isConnected).toBe(false);
    expect(localStorage.getItem(THEME_PACK_STORAGE_KEY)).toBe("matcha");
    expect(localStorage.getItem("zudo-doc-theme")).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(changes).toEqual([
      { pack: "foundry", previous: "default" },
      { pack: "matcha", previous: "foundry" },
    ]);

    window.removeEventListener(THEME_PACK_CHANGED_EVENT, onChange);
  });

  it("restores the saved pack before paint and repairs soft-navigation heads", () => {
    localStorage.setItem(THEME_PACK_STORAGE_KEY, "matcha");
    localStorage.setItem("zudo-doc-theme", "dark");
    document.documentElement.setAttribute("data-theme", "dark");
    document.documentElement.setAttribute(THEME_PACK_ATTR, "default");
    const readyState = vi
      .spyOn(document, "readyState", "get")
      .mockReturnValue("loading");

    let bootstrapLink: HTMLLinkElement | null = null;
    const append = vi
      .spyOn(document.head, "appendChild")
      .mockImplementation((node) => {
        bootstrapLink = node as HTMLLinkElement;
        return node;
      });
    window.eval(
      buildThemePackBootstrap("default", versions, "data:text/css,"),
    );

    expect(document.documentElement.getAttribute(THEME_PACK_ATTR)).toBe(
      "matcha",
    );
    expect(document.documentElement.hasAttribute(THEME_PACK_LOADING_ATTR)).toBe(
      true,
    );
    expect(bootstrapLink).not.toBeNull();
    expect(bootstrapLink!.getAttribute("href")).toContain(
      `theme-packs/matcha/pack.css?v=${versions.matcha}`,
    );
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(localStorage.getItem("zudo-doc-theme")).toBe("dark");

    bootstrapLink!.dispatchEvent(new Event("load"));
    expect(document.documentElement.hasAttribute(THEME_PACK_LOADING_ATTR)).toBe(
      false,
    );
    append.mockRestore();
    document.head.append(bootstrapLink!);

    const nextDocument = document.implementation.createHTMLDocument("next");
    const beforeSwap = new CustomEvent("zfb:before-swap") as CustomEvent & {
      newDocument?: Document;
    };
    beforeSwap.newDocument = nextDocument;
    document.dispatchEvent(beforeSwap);
    expect(
      nextDocument.head.querySelector<HTMLLinkElement>(
        `link[${THEME_PACK_LINK_ATTR}]`,
      )?.getAttribute("href"),
    ).toBe(bootstrapLink!.getAttribute("href"));

    document.documentElement.removeAttribute(THEME_PACK_ATTR);
    document.dispatchEvent(new Event("zfb:after-swap"));
    expect(document.documentElement.getAttribute(THEME_PACK_ATTR)).toBe(
      "matcha",
    );
    expect(document.head.querySelectorAll(`link[${THEME_PACK_LINK_ATTR}]`))
      .toHaveLength(1);
    readyState.mockRestore();
  });

  it("fetches the staged catalog only when browse-all opens", async () => {
    document.documentElement.setAttribute(THEME_PACK_ATTR, "default");
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue({
      ok: true,
      json: async () => themePackCatalog,
    } as Response);
    const root = document.createElement("div");
    document.body.append(root);
    const order = themePackCatalog.packs.map(
      ({ slug, name, mode, description }) => ({
        slug,
        name,
        mode,
        description,
      }),
    );
    act(() => render(
      <ThemePackSwitcher active="default" order={order} base="/" />,
      root,
    ));

    expect(fetchMock).not.toHaveBeenCalled();
    click(root.querySelector('[data-switcher-launcher]')!);
    click(root.querySelector('[aria-label="Browse all theme packs"]')!);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).toHaveBeenCalledWith("/theme-packs/index.json");
    act(() => render(null, root));
    root.remove();
  });

  it("uses a storage key and event independent from the light/dark toggle", () => {
    expect(THEME_PACK_STORAGE_KEY).toBe("zudo-doc-theme-pack");
    expect(THEME_PACK_CHANGED_EVENT).toBe("theme-pack-changed");
    document.documentElement.setAttribute("data-theme", "dark");
    const root = document.createElement("div");
    document.body.append(root);
    act(() => render(<ThemeToggle defaultMode="dark" />, root));
    click(root.querySelector('[aria-label="Switch to light mode"]')!);

    expect(localStorage.getItem("zudo-doc-theme")).toBe("light");
    expect(localStorage.getItem(THEME_PACK_STORAGE_KEY)).toBeNull();
    act(() => render(null, root));
    root.remove();
  });
});
