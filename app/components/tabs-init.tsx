/**
 * TabsInit — click handler for tab containers rendered server-side.
 *
 * Ported from $HOME/.claude/doc/src/components/tabs-init.astro.
 * Targets [data-tabs] containers with .tabs-nav and .tab-panel children.
 * CSS classes on buttons are the same as in the original (inline Tailwind
 * classes from S4 token set). Groups synced via localStorage keyed on
 * data-group-id.
 *
 * In zfb, ViewTransitions triggers real page loads so the script runs
 * fresh on every navigation — DOMContentLoaded is sufficient.
 */

const SCRIPT = `(function () {
  function getButtonClasses(isActive) {
    var base = "px-hsp-lg py-vsp-xs text-small font-medium border-b-[5px] -mb-px transition-colors";
    return isActive
      ? base + " text-accent border-accent"
      : base + " text-muted border-transparent hover:text-fg";
  }

  function showPanel(container, value) {
    var panels = container.querySelectorAll(".tab-panel");
    for (var i = 0; i < panels.length; i++) {
      panels[i].hidden = panels[i].dataset.tabValue !== value;
    }
  }

  function activateTab(container, value) {
    var buttons = container.querySelectorAll("[data-tab-btn]");
    for (var i = 0; i < buttons.length; i++) {
      var isActive = buttons[i].dataset.tabBtn === value;
      buttons[i].className = getButtonClasses(isActive);
      buttons[i].setAttribute("aria-selected", String(isActive));
    }
    showPanel(container, value);
  }

  function syncGroup(groupId, value, source) {
    var others = document.querySelectorAll('[data-tabs][data-group-id="' + CSS.escape(groupId) + '"]');
    for (var i = 0; i < others.length; i++) {
      if (others[i] === source) continue;
      var hasPanel = others[i].querySelector('.tab-panel[data-tab-value="' + CSS.escape(value) + '"]');
      if (hasPanel) {
        activateTab(others[i], value);
      }
    }
  }

  function initTabs() {
    var containers = document.querySelectorAll("[data-tabs]");

    for (var i = 0; i < containers.length; i++) {
      var container = containers[i];
      if (container.dataset.tabsInit) continue;
      container.dataset.tabsInit = "true";

      var nav = container.querySelector(".tabs-nav");
      var panels = container.querySelectorAll(".tab-panel");
      if (!nav || panels.length === 0) continue;

      var groupId = container.dataset.groupId;

      // Determine which tab should be active initially
      var activeValue = null;

      // Check localStorage for grouped tabs
      if (groupId) {
        try {
          var stored = localStorage.getItem("tabs-group-" + groupId);
          if (stored) {
            var hasStored = Array.from(panels).some(function (p) {
              return p.dataset.tabValue === stored;
            });
            if (hasStored) activeValue = stored;
          }
        } catch (e) {}
      }

      // Fall back to default panel or first panel
      if (!activeValue) {
        var defaultPanel = container.querySelector(".tab-panel[data-tab-default]");
        activeValue = defaultPanel
          ? defaultPanel.dataset.tabValue
          : panels[0].dataset.tabValue;
      }

      // Create buttons for each panel
      (function (container, groupId, activeValue) {
        var panels = container.querySelectorAll(".tab-panel");
        for (var j = 0; j < panels.length; j++) {
          (function (panel, container, groupId, activeValue) {
            var value = panel.dataset.tabValue;
            var label = panel.dataset.tabLabel;
            var btn = document.createElement("button");
            btn.type = "button";
            btn.role = "tab";
            btn.textContent = label;
            btn.dataset.tabBtn = value;
            btn.className = getButtonClasses(value === activeValue);
            btn.setAttribute("aria-selected", String(value === activeValue));

            btn.addEventListener("click", function () {
              activateTab(container, value);
              if (groupId) {
                try { localStorage.setItem("tabs-group-" + groupId, value); } catch (e) {}
                syncGroup(groupId, value, container);
              }
            });

            var nav = container.querySelector(".tabs-nav");
            nav.appendChild(btn);
          })(panels[j], container, groupId, activeValue);
        }
      })(container, groupId, activeValue);

      // Show the active panel
      showPanel(container, activeValue);
    }
  }

  // Run on DOMContentLoaded (or immediately if already loaded)
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", initTabs);
  } else {
    initTabs();
  }
})();`;

export default function TabsInit() {
  return <script dangerouslySetInnerHTML={{ __html: SCRIPT }} />;
}
