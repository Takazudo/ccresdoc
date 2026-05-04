import type { ComponentChildren } from "preact";

import Header from "../components/header";
import Footer from "../components/footer";
import SidebarMount from "../components/sidebar-mount";
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
 * Inline script to restore sidebar width from localStorage before paint,
 * so layout does not jump when the CSS custom property is updated by JS.
 */
const SIDEBAR_WIDTH_SCRIPT = `(() => {
  try {
    var w = localStorage.getItem("ccresdoc.sidebarWidth");
    if (w && /^\\d+$/.test(w)) {
      document.documentElement.style.setProperty("--ccresdoc-sidebar-width", w + "px");
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
        {/* Apply theme and sidebar width before paint to avoid FOUC. */}
        <script dangerouslySetInnerHTML={{ __html: THEME_BOOTSTRAP_SCRIPT }} />
        {/* Inject --zd-* palette variables for the active data-theme. */}
        <ColorSchemeProvider />
        <script dangerouslySetInnerHTML={{ __html: SIDEBAR_WIDTH_SCRIPT }} />
      </head>
      <body>
        <div class="ccresdoc-shell" data-sidebar-open="true">
          <Header />
          <SidebarMount />
          <main class="ccresdoc-main">{children}</main>
          <Footer />
        </div>
      </body>
    </html>
  );
}
