/** @jsxRuntime automatic */
/** @jsxImportSource preact */
// Head slot builder for CCResDoc doc pages.
// Emits ColorSchemeProvider (theme bootstrap + CSS vars) + DocHead meta.

import type { JSX } from "preact";
import { DocHead } from "@takazudo/zudo-doc/head";
// Import ColorSchemeProvider from the dedicated subpath, NOT the
// "@takazudo/zudo-doc/theme" barrel — the barrel also re-exports the
// ColorTweakExportModal / design-token-serde modules, whose types reference
// the (now removed) @takazudo/zdtp panel and which this SSR-only head does
// not need in its zfb esbuild graph. Mirrors zudo-doc's own scaffold.
import ColorSchemeProvider from "@takazudo/zudo-doc/theme/color-scheme-provider";
import { SIDEBAR_RESIZER_RESTORE_SCRIPT } from "@takazudo/zudo-doc/sidebar-resizer";
import { settings } from "@/config/settings";
import { colorSchemeCssText } from "@/config/color-scheme-utils";

interface HeadWithDefaultsProps {
  title: string;
  description?: string;
  noindex?: boolean;
  canonical?: string;
}

export function HeadWithDefaults({
  title,
  description,
  noindex,
  canonical,
}: HeadWithDefaultsProps): JSX.Element {
  return (
    <>
      {settings.colorMode && (
        <ColorSchemeProvider
          cssText={colorSchemeCssText}
          colorMode={{
            defaultMode: settings.colorMode.defaultMode,
            respectPrefersColorScheme: settings.colorMode.respectPrefersColorScheme,
          }}
        />
      )}
      {/* Pre-paint inline script: restore the persisted sidebar width to
          --zd-sidebar-w on :root before first paint, so a reload after
          drag-resizing doesn't snap back to the CSS default clamp() width. */}
      {settings.sidebarResizer && (
        <script dangerouslySetInnerHTML={{ __html: SIDEBAR_RESIZER_RESTORE_SCRIPT }} />
      )}
      <DocHead
        title={title}
        description={description}
        noindex={noindex ?? settings.noindex}
        canonical={canonical}
      />
    </>
  );
}
