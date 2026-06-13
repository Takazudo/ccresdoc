/** @jsxRuntime automatic */
/** @jsxImportSource preact */
// CCResDoc 404 page.

import type { JSX } from "preact";
import { DocLayoutWithDefaults } from "@takazudo/zudo-doc/doclayout";
import { HeadWithDefaults } from "./lib/_head-with-defaults";
import { HeaderWithDefaults } from "./lib/_header-with-defaults";
import { FooterWithDefaults } from "./lib/_footer-with-defaults";
import { BodyEndIslands } from "./lib/_body-end-islands";
import { withBase } from "@/utils/base";
import { settings } from "@/config/settings";

export const frontmatter = {
  title: "404 — Page not found",
  standalone: true,
};

export default function NotFoundPage(): JSX.Element {
  const title = `404 — Page not found | ${settings.siteName}`;

  return (
    <DocLayoutWithDefaults
      title={title}
      lang="en"
      head={
        <HeadWithDefaults
          title={title}
          noindex
        />
      }
      headerOverride={<HeaderWithDefaults lang="en" />}
      footerOverride={<FooterWithDefaults />}
      bodyEndComponents={<BodyEndIslands />}
      hideSidebar
      hideToc
    >
      <div class="py-vsp-xl px-hsp-lg">
        <h1 class="text-4xl font-bold mb-vsp-md">404</h1>
        <p class="text-muted mb-vsp-lg">The page you are looking for does not exist.</p>
        <a
          href={withBase("/")}
          class="text-accent hover:underline"
        >
          Return to the home page
        </a>
      </div>
    </DocLayoutWithDefaults>
  );
}
