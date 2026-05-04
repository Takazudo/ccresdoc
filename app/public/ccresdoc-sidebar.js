/**
 * ccresdoc-sidebar.js
 *
 * Browser script that:
 *  1. Fetches /api/manifest.json (S5 endpoint) and hydrates #ccresdoc-sidebar.
 *  2. Wires up the sidebar toggle (localStorage key: ccresdoc.sidebarOpen).
 *  3. Wires up the sidebar resizer (localStorage key: ccresdoc.sidebarWidth,
 *     CSS custom property: --ccresdoc-sidebar-width, min 200px, max 480px).
 *
 * All behaviours are progressive enhancement — if this script fails to load
 * or errors, the sidebar renders at its CSS default width and remains visible.
 */

(function () {
  "use strict";

  // ─── Manifest types ───────────────────────────────────────────────────────
  // @typedef {{ generatedAt: string, categories: Array<{slug: string, label: string, items: Array<{slug: string, label: string, path: string}>}> }} Manifest

  var SIDEBAR_OPEN_KEY = "ccresdoc.sidebarOpen";
  var SIDEBAR_WIDTH_KEY = "ccresdoc.sidebarWidth";
  var SIDEBAR_MIN_WIDTH = 200;
  var SIDEBAR_MAX_WIDTH = 480;

  // ─── DOM helpers ──────────────────────────────────────────────────────────

  function getShell() {
    return document.querySelector(".ccresdoc-shell");
  }

  function getSidebarNav() {
    return document.getElementById("ccresdoc-sidebar");
  }

  // ─── Sidebar toggle ───────────────────────────────────────────────────────

  function readSidebarOpen() {
    try {
      var v = localStorage.getItem(SIDEBAR_OPEN_KEY);
      if (v === "false") return false;
    } catch (_) {}
    return true; // default open
  }

  function writeSidebarOpen(isOpen) {
    try {
      localStorage.setItem(SIDEBAR_OPEN_KEY, isOpen ? "true" : "false");
    } catch (_) {}
  }

  function applySidebarOpen(isOpen) {
    var shell = getShell();
    if (!shell) return;
    shell.dataset.sidebarOpen = isOpen ? "true" : "false";
    var btn = document.getElementById("ccresdoc-sidebar-toggle");
    if (btn) {
      btn.setAttribute("aria-expanded", isOpen ? "true" : "false");
    }
  }

  function initToggle() {
    var btn = document.getElementById("ccresdoc-sidebar-toggle");
    if (!btn) return;

    // Apply persisted state on load.
    var isOpen = readSidebarOpen();
    applySidebarOpen(isOpen);

    btn.addEventListener("click", function () {
      var shell = getShell();
      if (!shell) return;
      var current = shell.dataset.sidebarOpen !== "false";
      var next = !current;
      applySidebarOpen(next);
      writeSidebarOpen(next);
    });
  }

  // ─── Sidebar resizer ──────────────────────────────────────────────────────

  function readSidebarWidth() {
    try {
      var v = localStorage.getItem(SIDEBAR_WIDTH_KEY);
      if (v) {
        var n = parseInt(v, 10);
        if (n >= SIDEBAR_MIN_WIDTH && n <= SIDEBAR_MAX_WIDTH) return n;
      }
    } catch (_) {}
    return null;
  }

  function writeSidebarWidth(px) {
    try {
      localStorage.setItem(SIDEBAR_WIDTH_KEY, String(px));
    } catch (_) {}
  }

  function applySidebarWidth(px) {
    document.documentElement.style.setProperty("--ccresdoc-sidebar-width", px + "px");
  }

  function initResizer() {
    var handle = document.getElementById("ccresdoc-sidebar-resizer");
    if (!handle) return;

    // Restore persisted width.
    var saved = readSidebarWidth();
    if (saved !== null) {
      applySidebarWidth(saved);
    }

    var dragging = false;
    var startX = 0;
    var startWidth = 0;

    function getCurrentWidth() {
      var style = getComputedStyle(document.documentElement);
      var val = style.getPropertyValue("--ccresdoc-sidebar-width");
      return parseInt(val, 10) || 280;
    }

    handle.addEventListener("mousedown", function (e) {
      dragging = true;
      startX = e.clientX;
      startWidth = getCurrentWidth();
      handle.classList.add("dragging");
      document.body.style.userSelect = "none";
      document.body.style.cursor = "col-resize";
    });

    document.addEventListener("mousemove", function (e) {
      if (!dragging) return;
      var delta = e.clientX - startX;
      var newWidth = Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, startWidth + delta));
      applySidebarWidth(newWidth);
    });

    document.addEventListener("mouseup", function () {
      if (!dragging) return;
      dragging = false;
      handle.classList.remove("dragging");
      document.body.style.userSelect = "";
      document.body.style.cursor = "";
      var finalWidth = getCurrentWidth();
      writeSidebarWidth(finalWidth);
    });

    // Touch support
    handle.addEventListener("touchstart", function (e) {
      var touch = e.touches[0];
      if (!touch) return;
      dragging = true;
      startX = touch.clientX;
      startWidth = getCurrentWidth();
      handle.classList.add("dragging");
    }, { passive: true });

    document.addEventListener("touchmove", function (e) {
      if (!dragging) return;
      var touch = e.touches[0];
      if (!touch) return;
      var delta = touch.clientX - startX;
      var newWidth = Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, startWidth + delta));
      applySidebarWidth(newWidth);
    }, { passive: true });

    document.addEventListener("touchend", function () {
      if (!dragging) return;
      dragging = false;
      handle.classList.remove("dragging");
      var finalWidth = getCurrentWidth();
      writeSidebarWidth(finalWidth);
    });
  }

  // ─── Manifest fetch + sidebar hydration ───────────────────────────────────

  function buildSidebarHTML(manifest) {
    var currentPath = location.pathname;
    var sections = manifest.categories.map(function (cat) {
      var items = cat.items.map(function (item) {
        var isActive = currentPath === item.path || currentPath === item.path + "/";
        var activeClass = isActive ? " active" : "";
        return (
          '<li><a href="' +
          escapeAttr(item.path) +
          '" class="' +
          activeClass.trim() +
          '">' +
          escapeHtml(item.label) +
          "</a></li>"
        );
      }).join("");

      return (
        '<div class="ccresdoc-sidebar-section">' +
        '<div class="ccresdoc-sidebar-section-title">' +
        escapeHtml(cat.label) +
        "</div>" +
        '<ul class="ccresdoc-sidebar-list">' +
        items +
        "</ul>" +
        "</div>"
      );
    });
    return sections.join("");
  }

  function escapeHtml(str) {
    return String(str)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  function escapeAttr(str) {
    return String(str)
      .replace(/&/g, "&amp;")
      .replace(/"/g, "&quot;");
  }

  function hydrateSidebar(manifest) {
    var nav = getSidebarNav();
    if (!nav) return;
    nav.innerHTML = buildSidebarHTML(manifest);
  }

  function fetchManifest() {
    fetch("/api/manifest.json")
      .then(function (res) {
        if (!res.ok) throw new Error("manifest fetch failed: " + res.status);
        return res.json();
      })
      .then(function (manifest) {
        hydrateSidebar(manifest);
      })
      .catch(function (err) {
        // Silent failure — sidebar stays empty but the page still works.
        console.warn("[ccresdoc-sidebar] manifest unavailable:", err.message);
      });
  }

  // ─── Init ─────────────────────────────────────────────────────────────────

  function init() {
    initToggle();
    initResizer();
    fetchManifest();
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
