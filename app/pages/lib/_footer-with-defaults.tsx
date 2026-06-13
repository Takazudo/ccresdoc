/** @jsxRuntime automatic */
/** @jsxImportSource preact */
// Minimal footer for CCResDoc (no link columns, no tags).

import type { JSX } from "preact";
import { Footer } from "@takazudo/zudo-doc/footer";
import { settings } from "@/config/settings";

export function FooterWithDefaults(): JSX.Element {
  const footer = settings.footer;
  if (!footer) {
    return <Footer persistKey="footer-en" />;
  }
  return (
    <Footer
      persistKey="footer-en"
      copyright={footer.copyright}
    />
  );
}
