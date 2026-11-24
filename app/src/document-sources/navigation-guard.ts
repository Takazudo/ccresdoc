/**
 * zfb's before-preparation event cannot veto a navigation: preventDefault()
 * delegates to a full document load. Guard links before the router sees them,
 * and browser traversals at capture-phase popstate before its bubble listener.
 */
export function installUnsavedNavigationGuard({
  win, doc, isDirty, confirmLeave,
}: {
  win: Window;
  doc: Document;
  isDirty: () => boolean;
  confirmLeave: () => boolean;
}): () => void {
  let currentIndex = Number(win.history.state?.index);
  let restoring = false;
  const accept = () => !isDirty() || confirmLeave();
  const onLink = (event: MouseEvent) => {
    if (!isDirty()) return;
    const anchor = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>("a[href]") : null;
    if (!anchor || anchor.getAttribute("href")?.startsWith("#")) return;
    if (!accept()) {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  };
  const onCommand = (event: Event) => {
    if (!accept()) event.preventDefault();
  };
  const onPopState = (event: PopStateEvent) => {
    if (restoring) {
      restoring = false;
      event.stopImmediatePropagation();
      return;
    }
    const nextIndex = Number(event.state?.index);
    if (accept()) { currentIndex = nextIndex; return; }
    event.stopImmediatePropagation();
    // The browser has already moved before popstate fires. Return to the
    // editor's entry without allowing zfb to swap or destroy its draft.
    restoring = true;
    const delta = Number.isFinite(currentIndex) && Number.isFinite(nextIndex)
      ? currentIndex - nextIndex
      : 1;
    win.history.go(delta || 1);
  };
  doc.addEventListener("click", onLink, true);
  doc.addEventListener("ccresdoc:before-document-navigation", onCommand);
  win.addEventListener("popstate", onPopState, true);
  return () => {
    doc.removeEventListener("click", onLink, true);
    doc.removeEventListener("ccresdoc:before-document-navigation", onCommand);
    win.removeEventListener("popstate", onPopState, true);
  };
}
