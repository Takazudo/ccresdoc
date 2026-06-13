/** @jsxRuntime automatic */
/** @jsxImportSource preact */
// Head slot builder for CCResDoc doc pages.
// Emits ColorSchemeProvider (theme bootstrap + CSS vars) + DocHead meta.

import type { JSX } from "preact";
import { DocHead } from "@takazudo/zudo-doc/head";
import { ColorSchemeProvider } from "@takazudo/zudo-doc/theme";
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
      <DocHead
        title={title}
        description={description}
        noindex={noindex ?? settings.noindex}
        canonical={canonical}
      />
    </>
  );
}
