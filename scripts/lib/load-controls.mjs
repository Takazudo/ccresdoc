// The JS-only header controls that zudo-doc renders with `data-zd-pending`
// until their island activates. Readiness is the package's own pending
// attribute clearing on every one of them; CCResDoc owns no readiness marker.
export const LOAD_CONTROL_SELECTORS = Object.freeze([
  '[data-zfb-island="ThemeToggle"] button',
  '[data-zfb-island="ThemePackSwitcher"] [data-switcher-launcher]',
  '[data-zfb-island="SettingsHeaderButton"] button',
]);

/** Browser-side predicate; self-contained so Playwright can serialize it. */
export function loadControlsSettled(selectors) {
  return selectors.every((selector) => {
    const controls = document.querySelectorAll(selector);
    return controls.length > 0 && [...controls].every((control) => !control.hasAttribute("data-zd-pending"));
  });
}

export function waitForLoadControls(page, timeout) {
  return page.waitForFunction(loadControlsSettled, LOAD_CONTROL_SELECTORS, { timeout });
}
