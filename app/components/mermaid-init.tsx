/**
 * MermaidInit — client-side mermaid diagram renderer.
 *
 * Ported from $HOME/.claude/doc/src/components/mermaid-init.astro.
 * The original used Astro's astro:page-load event for SPA navigation.
 * In zfb, ViewTransitions triggers real page loads so the script runs
 * fresh on every navigation — DOMContentLoaded is sufficient.
 *
 * Observes mutations on html[style] to re-render when the color scheme
 * changes (theme toggle updates CSS custom properties on <html>).
 */

const SCRIPT = `(function () {
  function resolveColor(value) {
    if (!value) return value;
    if (/^#[0-9a-fA-F]{3}$/.test(value)) {
      return "#" + value[1] + value[1] + value[2] + value[2] + value[3] + value[3];
    }
    if (/^#[0-9a-fA-F]{6}$/.test(value)) return value;
    if (/^#[0-9a-fA-F]{8}$/.test(value)) return value.slice(0, 7);
    if (/^#[0-9a-fA-F]{4}$/.test(value)) {
      return "#" + value[1] + value[1] + value[2] + value[2] + value[3] + value[3];
    }
    var el = document.createElement("div");
    el.style.display = "none";
    el.style.color = value;
    document.body.appendChild(el);
    var resolved;
    try {
      resolved = getComputedStyle(el).color;
    } finally {
      el.remove();
    }
    var m = resolved.match(/(\\d+)/g);
    if (m && m.length >= 3) {
      return "#" + m.slice(0, 3).map(function (n) {
        return Number(n).toString(16).padStart(2, "0");
      }).join("");
    }
    return value;
  }

  function initMermaid() {
    var els = document.querySelectorAll("[data-mermaid]:not([data-mermaid-rendered])");
    if (els.length === 0) return;

    import("mermaid").then(function (mod) {
      var mermaid = mod.default;
      var s = getComputedStyle(document.documentElement);
      var v = function (name) { return resolveColor(s.getPropertyValue(name).trim()); };
      var bg = v("--zd-bg");
      var hex = bg.replace("#", "");
      var r = parseInt(hex.substring(0, 2), 16) / 255;
      var g = parseInt(hex.substring(2, 4), 16) / 255;
      var b = parseInt(hex.substring(4, 6), 16) / 255;
      var luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
      mermaid.initialize({
        startOnLoad: false,
        theme: "base",
        themeVariables: {
          darkMode: isNaN(luminance) ? true : luminance < 0.5,
          background: "transparent",
          primaryColor: v("--zd-mermaid-node-bg"),
          primaryTextColor: v("--zd-mermaid-text"),
          primaryBorderColor: v("--zd-mermaid-line"),
          lineColor: v("--zd-mermaid-line"),
          secondaryColor: v("--zd-mermaid-note-bg"),
          tertiaryColor: v("--zd-mermaid-note-bg"),
          edgeLabelBackground: v("--zd-mermaid-label-bg"),
          labelTextColor: v("--zd-mermaid-text"),
          transitionColor: v("--zd-mermaid-line"),
          transitionLabelColor: v("--zd-mermaid-text"),
          stateLabelColor: v("--zd-mermaid-text"),
          noteBkgColor: v("--zd-mermaid-note-bg"),
          noteTextColor: v("--zd-mermaid-text"),
          noteBorderColor: v("--zd-mermaid-line"),
          fontFamily: "inherit",
        },
      });
      return mermaid.run({ nodes: Array.from(els) }).then(function () {
        els.forEach(function (el) { el.setAttribute("data-mermaid-rendered", ""); });
      });
    }).catch(function (e) {
      console.error("[mermaid-init] Failed to render mermaid diagrams:", e);
    });
  }

  function reinitMermaid() {
    document.querySelectorAll("[data-mermaid-rendered]").forEach(function (el) {
      el.removeAttribute("data-mermaid-rendered");
      var svg = el.querySelector("svg");
      if (svg) svg.remove();
    });
    initMermaid();
  }

  // Run on DOMContentLoaded (or immediately if already loaded)
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", initMermaid);
  } else {
    initMermaid();
  }

  // Re-render mermaid when color scheme changes (debounced)
  var tweakTimer;
  new MutationObserver(function () {
    clearTimeout(tweakTimer);
    tweakTimer = setTimeout(reinitMermaid, 300);
  }).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["style"],
  });
})();`;

export default function MermaidInit() {
  return <script dangerouslySetInnerHTML={{ __html: SCRIPT }} />;
}
