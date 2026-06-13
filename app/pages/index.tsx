/** @jsxRuntime automatic */
/** @jsxImportSource preact */
// CCResDoc home page.
//
// Simple landing page; the primary content is at /docs/claude/ (Wave 2
// Rust generator populates ~/.claude/ resources there).

import type { JSX } from "preact";
import { DocLayoutWithDefaults } from "@takazudo/zudo-doc/doclayout";
import { HeadWithDefaults } from "./lib/_head-with-defaults";
import { HeaderWithDefaults } from "./lib/_header-with-defaults";
import { FooterWithDefaults } from "./lib/_footer-with-defaults";
import { BodyEndIslands } from "./lib/_body-end-islands";
import { withBase } from "@/utils/base";
import { settings } from "@/config/settings";

export const frontmatter = {
  title: "CCResDoc",
  standalone: true,
};

export default function HomePage(): JSX.Element {
  const docsHref = withBase("/docs/claude/");

  return (
    <DocLayoutWithDefaults
      title={settings.siteName}
      lang="en"
      head={
        <HeadWithDefaults
          title={settings.siteName}
          description={settings.siteDescription}
        />
      }
      headerOverride={<HeaderWithDefaults lang="en" />}
      footerOverride={<FooterWithDefaults />}
      bodyEndComponents={<BodyEndIslands />}
      hideSidebar
      hideToc
    >
      <div class="py-vsp-xl px-hsp-lg text-center">
        <h1 class="text-4xl font-bold mb-vsp-md">{settings.siteName}</h1>
        <p class="text-muted mb-vsp-lg max-w-prose mx-auto">
          {settings.siteDescription}
        </p>
        <a
          href={docsHref}
          class="inline-block bg-accent text-bg px-hsp-lg py-vsp-sm rounded hover:bg-accent-hover transition-colors"
        >
          Browse Claude Resources
        </a>
      </div>
    </DocLayoutWithDefaults>
  );
}
