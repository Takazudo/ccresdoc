/**
 * PageLoadingOverlay — full-page spinner overlay during slow page transitions.
 *
 * Ported from $HOME/.claude/doc/src/components/page-loading-overlay.astro.
 * The original used astro:before-preparation / astro:page-load lifecycle
 * events for Astro's SPA-style view transitions.
 *
 * In zfb, ViewTransitions triggers real browser page loads (location.href
 * assignment inside startViewTransition). The browser's own navigation
 * progress bar is visible, so there is no pre-swap moment to hook into.
 * This component emits the overlay DOM and styles so CSS from global.css
 * can target .page-loading-overlay, but the JS hook is intentionally a
 * no-op in the zfb context (real navigations get browser-native loading
 * feedback). The overlay remains hidden (opacity: 0, pointer-events: none)
 * at all times — it is kept in the DOM so any future hook-point added to
 * zfb can enable it without a structural change.
 */

const STYLES = `.page-loading-overlay {
  position: fixed;
  inset: 0;
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.6);
  opacity: 0;
  pointer-events: none;
  transition: opacity 150ms ease-out;
}

.page-loading-overlay[data-visible] {
  opacity: 1;
  pointer-events: auto;
}

.page-loading-spinner {
  width: 48px;
  height: 48px;
  border: 5px solid var(--color-fg, #fff);
  border-bottom-color: transparent;
  border-radius: 50%;
  display: inline-block;
  box-sizing: border-box;
  animation: page-loading-spin 1s linear infinite;
}

@media (min-width: 1024px) {
  .page-loading-spinner {
    width: 64px;
    height: 64px;
    border-width: 6px;
  }
}

@keyframes page-loading-spin {
  0% { transform: rotate(0deg); }
  100% { transform: rotate(360deg); }
}

@media (prefers-reduced-motion: reduce) {
  .page-loading-spinner {
    animation: none;
    border-bottom-color: var(--color-fg, #fff);
    opacity: 0.5;
  }
}`;

export default function PageLoadingOverlay() {
  return (
    <>
      <style dangerouslySetInnerHTML={{ __html: STYLES }} />
      <div
        id="page-loading-overlay"
        class="page-loading-overlay"
        aria-hidden="true"
      >
        <span class="page-loading-spinner" />
      </div>
    </>
  );
}
