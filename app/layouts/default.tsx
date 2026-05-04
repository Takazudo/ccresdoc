import type { ComponentChildren } from "preact";

import { Island } from "@takazudo/zfb";
import Header from "../components/header";
import Footer from "../components/footer";
import Sidebar from "../components/sidebar";
import SidebarToggle from "../components/sidebar-toggle";
import DesktopSidebarToggle from "../components/desktop-sidebar-toggle";
import ColorSchemeProvider from "../components/color-scheme-provider";
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

type Props = {
  title: string;
  children: ComponentChildren;
};

export default function DefaultLayout({ title, children }: Props) {
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
        {/* Restore sidebar visibility state before paint. */}
        <script dangerouslySetInnerHTML={{ __html: SIDEBAR_VISIBILITY_SCRIPT }} />
      </head>
      <body>
        <div class="ccresdoc-shell">
          {/* Desktop sidebar — fixed left column, hidden by data-sidebar-hidden */}
          <div
            id="desktop-sidebar"
            class="hidden lg:flex fixed top-[3.5rem] left-0 h-[calc(100vh-3.5rem)] flex-col border-r border-muted bg-surface overflow-y-auto"
            style={{ width: "var(--zd-sidebar-w)" }}
          >
            <Island when="idle">
              <Sidebar />
            </Island>
          </div>

          {/* Desktop sidebar toggle — fixed button at sidebar edge */}
          <Island when="idle">
            <DesktopSidebarToggle />
          </Island>

          {/* Main content area — offset by sidebar width on desktop */}
          <div class="zd-sidebar-content-wrapper lg:ml-[var(--zd-sidebar-w)]">
            {/* Header includes mobile sidebar toggle with Sidebar inside */}
            <Header />

            {/* Mobile sidebar toggle wrapper — wraps the sidebar for mobile slide-in */}
            <Island when="idle">
              <SidebarToggle>
                <Sidebar />
              </SidebarToggle>
            </Island>

            <main class="ccresdoc-main">{children}</main>
            <Footer />
          </div>
        </div>
      </body>
    </html>
  );
}
