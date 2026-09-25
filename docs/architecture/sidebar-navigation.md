# Sidebar and navigation ownership

CCResDoc uses the public navigation surface in `@takazudo/zudo-doc` 5.27.0.
The package owns sidebar rendering, filtering, persisted disclosure state,
soft-navigation active state, mobile and desktop toggles, theme controls, smart
path wrapping, and tree connector geometry. CCResDoc does not keep local copies
of those implementations and does not need a generated-category adapter: the
package `site-schema`, `nav-scope`, and `sidebar-utils` contracts represent the
generated resource hierarchy directly.

The integration is guarded by `app/test/navigation-islands.test.tsx`. It imports
the public package entry points (rather than package internals), exercises their
DOM behavior in happy-dom, and verifies duplicate-safe lifecycle cleanup.

## Accessibility contract and upstream deviation

zudo-doc 5.27.0 exposes the sidebar as native links and disclosure buttons. The
controls remain in the normal tab order and publish `aria-current`,
`aria-expanded`, and descriptive labels. This is an accessible disclosure/link
pattern, but it is not the WAI-ARIA tree pattern: the current public island does
not emit `role="tree"` / `role="treeitem"`, controlled-region IDs with
`aria-controls`, implement roving tabindex, or handle Arrow/Home/End at the tree
level. The removed CCResDoc mirror did implement that separate pattern, but
retaining its rendering and keyboard state machine would fork the upstream
component again. The DOM test records the difference so a future upstream
implementation change is reviewed explicitly. Focus restoration to the
hamburger after a navigation-driven mobile close is likewise not part of the
5.27.0 public island contract and should be addressed upstream rather than in a
host wrapper.

Re-checked at 5.27.0: the public island surface did not gain a new navigation
design. The published package still has `sidebar-toggle-island`,
`sidebar-tree-island`, and `sidebar-with-defaults`. The scan also covered
`site-tree-nav-island`, `tree-nav-shared`, and `desktop-sidebar-toggle-island`.
Those dist files still have no `role="tree"` / `role="treeitem"`, no
`aria-controls`, and no `tabIndex` or roving tabindex. The islands that publish
labels still use only `aria-current`, `aria-expanded`, `aria-hidden`, and
`aria-label`. The disclosure/link pattern and the deviation recorded above are
unchanged.

The 5.26.3–5.26.4 drawer fixes are behavior inside `sidebar-toggle-island`.
5.26.3 hides the inactive icon with an inline `display:none`, so an unlayered
consumer reset cannot paint the hamburger and the X together. 5.26.4 closes
the open drawer on Escape and returns focus to the toggle; while the drawer is
open the toggle uses `relative z-modal` so the X sits above the backdrop.
Escape is listened for only while the drawer is open. A navigation-driven
close still does not restore focus.

## Browser verification handoff

The manager's browser pass should check both light and dark modes at desktop
1440x900 and narrow 390x844:

- tab through sidebar links, disclosure buttons, theme toggle, and mobile menu;
- confirm visible focus rings and native Enter/Space activation;
- filter the tree, navigate without a reload, and confirm the active path and
  open category follow the destination;
- close and reopen categories, reload, and confirm persisted open state;
- open the mobile drawer, confirm background scrolling is locked and the closed
  drawer is inert, then navigate and confirm it closes;
- resize/toggle the desktop sidebar and confirm connector lines remain aligned;
- inspect long generated resource paths for smart wrapping and verify the page
  has no horizontal overflow.

The viewport/focus/overflow portion is intentionally browser-only; happy-dom
does not perform layout or native keyboard activation.
