import { Island } from "@takazudo/zfb";
import ThemeToggle from "./theme-toggle";

export default function Header() {
  return (
    <header class="ccresdoc-header">
      <a class="ccresdoc-site-title" href="/">
        CCResDoc
      </a>
      <div class="ccresdoc-header-actions">
        {/*
          Sidebar toggle button — JS in ccresdoc-sidebar.js wires up the
          click handler to toggle data-sidebar-open on .ccresdoc-shell.
        */}
        <button
          type="button"
          class="ccresdoc-sidebar-toggle"
          id="ccresdoc-sidebar-toggle"
          aria-label="Toggle sidebar"
          aria-expanded="true"
        >
          ☰
        </button>
        <Island when="idle">
          <ThemeToggle />
        </Island>
      </div>
    </header>
  );
}
