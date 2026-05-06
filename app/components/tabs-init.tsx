/**
 * TabsInit — click handler for tab containers rendered server-side.
 *
 * Supports two markup patterns:
 *
 * Pattern A (original Astro tabs.astro / tab-item.astro):
 *   [data-tabs] container with .tabs-nav (empty) and .tab-panel[data-tab-value][data-tab-label] children.
 *   Buttons are created dynamically by this script and appended to .tabs-nav.
 *
 * Pattern B (ccresdoc-renderer):
 *   .tabs-container with .tabs-list[role="tablist"] containing pre-rendered
 *   .tabs-tab[role="tab"] buttons, and .tabs-panel[role="tabpanel"] panels.
 *   Buttons already exist in HTML — this script only wires click handlers.
 *   The aria-controls / id relationship in the rendered HTML drives panel lookup.
 *
 * In zfb, ViewTransitions triggers real page loads so the script runs
 * fresh on every navigation — DOMContentLoaded is sufficient.
 */

const SCRIPT = `(function () {
  // ── Pattern A helpers (original [data-tabs] markup) ──────────────────────

  function getButtonClasses(isActive) {
    var base = "px-hsp-lg py-vsp-xs text-small font-medium border-b-[5px] -mb-px transition-colors";
    return isActive
      ? base + " text-accent border-accent"
      : base + " text-muted border-transparent hover:text-fg";
  }

  function showPanelA(container, value) {
    var panels = container.querySelectorAll(".tab-panel");
    for (var i = 0; i < panels.length; i++) {
      panels[i].hidden = panels[i].dataset.tabValue !== value;
    }
  }

  function activateTabA(container, value) {
    var buttons = container.querySelectorAll("[data-tab-btn]");
    for (var i = 0; i < buttons.length; i++) {
      var isActive = buttons[i].dataset.tabBtn === value;
      buttons[i].className = getButtonClasses(isActive);
      buttons[i].setAttribute("aria-selected", String(isActive));
    }
    showPanelA(container, value);
  }

  function syncGroup(groupId, value, source) {
    var others = document.querySelectorAll('[data-tabs][data-group-id="' + CSS.escape(groupId) + '"]');
    for (var i = 0; i < others.length; i++) {
      if (others[i] === source) continue;
      var hasPanel = others[i].querySelector('.tab-panel[data-tab-value="' + CSS.escape(value) + '"]');
      if (hasPanel) {
        activateTabA(others[i], value);
      }
    }
  }

  function initTabsPatternA() {
    var containers = document.querySelectorAll("[data-tabs]");
    for (var i = 0; i < containers.length; i++) {
      var container = containers[i];
      if (container.dataset.tabsInit) continue;
      container.dataset.tabsInit = "true";

      var nav = container.querySelector(".tabs-nav");
      var panels = container.querySelectorAll(".tab-panel");
      if (!nav || panels.length === 0) continue;

      var groupId = container.dataset.groupId;
      var activeValue = null;

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

      if (!activeValue) {
        var defaultPanel = container.querySelector(".tab-panel[data-tab-default]");
        activeValue = defaultPanel
          ? defaultPanel.dataset.tabValue
          : panels[0].dataset.tabValue;
      }

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
              activateTabA(container, value);
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

      showPanelA(container, activeValue);
    }
  }

  // ── Pattern B helpers (ccresdoc-renderer .tabs-container markup) ──────────
  // Renderer pre-renders buttons in .tabs-list; panels use id/aria-controls.

  function activateTabB(container, activeBtn) {
    var buttons = container.querySelectorAll(".tabs-tab");
    for (var i = 0; i < buttons.length; i++) {
      var isActive = buttons[i] === activeBtn;
      buttons[i].setAttribute("aria-selected", String(isActive));
      // Update active visual class on the tab button
      if (isActive) {
        buttons[i].setAttribute("data-active", "");
      } else {
        buttons[i].removeAttribute("data-active");
      }
    }
    // Show/hide panels via aria-controls
    var panelId = activeBtn.getAttribute("aria-controls");
    var allPanels = container.querySelectorAll(".tabs-panel");
    for (var j = 0; j < allPanels.length; j++) {
      allPanels[j].hidden = allPanels[j].id !== panelId;
    }
  }

  function initTabsPatternB() {
    var containers = document.querySelectorAll(".tabs-container");
    for (var i = 0; i < containers.length; i++) {
      var container = containers[i];
      if (container.dataset.tabsInit) continue;
      container.dataset.tabsInit = "true";

      // Wire click handlers on existing .tabs-tab buttons
      var buttons = container.querySelectorAll(".tabs-tab");
      if (buttons.length === 0) continue;

      (function (container, buttons) {
        for (var j = 0; j < buttons.length; j++) {
          (function (btn, container) {
            btn.addEventListener("click", function () {
              activateTabB(container, btn);
            });
          })(buttons[j], container);
        }
      })(container, buttons);

      // Ensure first selected button matches hidden state on initial load
      var selectedBtn = container.querySelector(".tabs-tab[aria-selected='true']");
      if (selectedBtn) {
        activateTabB(container, selectedBtn);
      }
    }
  }

  function initTabs() {
    initTabsPatternA();
    initTabsPatternB();
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
