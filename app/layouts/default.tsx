import type { ComponentChildren } from "preact";

import { Island } from "@takazudo/zfb";
import Header from "../components/header";
import Footer from "../components/footer";
import Sidebar from "../components/sidebar";
import DesktopSidebarToggle from "../components/desktop-sidebar-toggle";
import ColorSchemeProvider from "../components/color-scheme-provider";
import { Toc, type Heading } from "../components/toc";
import { MobileToc } from "../components/mobile-toc";
import CodeBlockEnhancer from "../components/code-block-enhancer";
import TabsInit from "../components/tabs-init";
import MermaidInit from "../components/mermaid-init";
import PageLoadingOverlay from "../components/page-loading-overlay";
import "../styles/global.css";

/**
 * Inline pre-paint script — runs synchronously before the page paints to
 * resolve the theme preference and apply data-theme on <html>. This avoids
 * FOUC (flash of un-themed content). Must be self-contained and tiny.
 *
 * Resolution order:
 *  1. localStorage["ccresdoc.theme"] if set to "light" or "dark"
 *  2. prefers-color-scheme: light  →  "light"
 *  3. fallback → "dark"
 */
const THEME_BOOTSTRAP_SCRIPT = `(() => {
  try {
    var saved = localStorage.getItem("ccresdoc.theme");
    var theme;
    if (saved === "light" || saved === "dark") {
      theme = saved;
    } else if (typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia("(prefers-color-scheme: light)").matches) {
      theme = "light";
    } else {
      theme = "dark";
    }
    document.documentElement.dataset.theme = theme;
  } catch (e) {
    document.documentElement.dataset.theme = "dark";
  }
})();`;

/**
 * Inline script to restore sidebar visibility from localStorage before paint.
 * Applies data-sidebar-hidden attribute so the CSS transition does not flash.
 * Key matches SIDEBAR_STORAGE_KEY in desktop-sidebar-toggle.tsx.
 */
const SIDEBAR_VISIBILITY_SCRIPT = `(() => {
  try {
    var v = localStorage.getItem("zudo-doc-sidebar-visible");
    if (v === "false") {
      document.documentElement.setAttribute("data-sidebar-hidden", "");
    }
  } catch (e) {}
})();`;

/**
 * Inline script to restore sidebar width from localStorage before paint.
 * Prevents layout shift when a user has resized the sidebar.
 * Key matches the original doc-layout.astro sidebarResizer script.
 */
const SIDEBAR_WIDTH_SCRIPT = `(() => {
  try {
    var w = localStorage.getItem("zudo-doc-sidebar-width");
    if (w && !isNaN(Number(w))) document.documentElement.style.setProperty("--zd-sidebar-w", w + "px");
  } catch (e) {}
})();`;

type Props = {
  title: string;
  children: ComponentChildren;
  headings?: Heading[];
  hideSidebar?: boolean;
  hideToc?: boolean;
};

export default function DefaultLayout({
  title,
  children,
  headings = [],
  hideSidebar = false,
  hideToc = false,
}: Props) {
  return (
    <html lang="en">
      <head>
        <meta charSet="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>{title}</title>
        {/* Apply theme before paint to avoid FOUC. */}
        <script dangerouslySetInnerHTML={{ __html: THEME_BOOTSTRAP_SCRIPT }} />
        {/* Inject --zd-* palette variables for the active data-theme. */}
        <ColorSchemeProvider />
        {/* Restore sidebar width before paint to avoid layout shift. */}
        <script dangerouslySetInnerHTML={{ __html: SIDEBAR_WIDTH_SCRIPT }} />
        {/* Restore sidebar visibility state before paint. */}
        <script dangerouslySetInnerHTML={{ __html: SIDEBAR_VISIBILITY_SCRIPT }} />
      </head>
      <body class="min-h-screen antialiased">
        <Header />

        {/* Desktop sidebar — fixed left column, hidden by data-sidebar-hidden */}
        {!hideSidebar && (
          <aside
            id="desktop-sidebar"
            aria-label="Documentation sidebar"
            class="hidden lg:block fixed top-[3.5rem] left-0 z-30 w-[var(--zd-sidebar-w)] h-[calc(100vh-3.5rem)] overflow-y-auto bg-bg border-r border-muted pb-vsp-xl"
          >
            <Island when="idle">
              <Sidebar />
            </Island>
          </aside>
        )}
        {!hideSidebar && (
          <Island when="idle">
            <DesktopSidebarToggle />
          </Island>
        )}

        <div
          class={[
            !hideSidebar && "lg:ml-[var(--zd-sidebar-w)]",
            !hideSidebar && "zd-sidebar-content-wrapper",
          ]
            .filter(Boolean)
            .join(" ")}
        >
          <div class="flex min-h-[calc(100vh-3.5rem)] justify-center">
            <div
              class={[
                "flex w-full gap-[clamp(1.5rem,3vw,4rem)]",
                hideSidebar
                  ? "max-w-[80rem]"
                  : "max-w-[clamp(50rem,75vw,90rem)]",
              ].join(" ")}
            >
              {/* Main content */}
              <main class="flex-1 min-w-0 px-hsp-xl py-vsp-xl lg:px-hsp-2xl lg:py-vsp-2xl">
                {!hideToc && (
                  <Island when="idle">
                    <MobileToc headings={headings} title="On this page" />
                  </Island>
                )}
                <article class="zd-content max-w-none">{children}</article>
              </main>

              {/* Table of contents */}
              {!hideToc && (
                <Island when="idle">
                  <Toc headings={headings} />
                </Island>
              )}
            </div>
          </div>
          <Footer />
        </div>

        {/* Init scripts — run on each page load */}
        <CodeBlockEnhancer />
        <TabsInit />
        <MermaidInit />
        <PageLoadingOverlay />
      </body>
    </html>
  );
}
