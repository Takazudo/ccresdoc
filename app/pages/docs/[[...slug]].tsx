/** @jsxRuntime automatic */
/** @jsxImportSource preact */
// CCResDoc docs catch-all route.
//
// Optional catch-all [[...slug]] so the root docs/index.mdx builds at
// /docs/ (zero-segment slug) and all nested pages build at their
// respective paths (e.g. /docs/claude/index → /docs/claude/).
//
// paths() is synchronous per zfb ADR-004: getCollection() resolves from
// the pre-loaded ContentSnapshot. category_no_page index files produce no
// route (they render as non-linked sidebar headers).

import type { JSX } from "preact";
import { DocLayoutWithDefaults } from "@takazudo/zudo-doc/doclayout";
import { getDocs } from "../_data";
import { mdxComponents } from "../_mdx-components";
import { HeadWithDefaults } from "../lib/_head-with-defaults";
import { HeaderWithDefaults } from "../lib/_header-with-defaults";
import { FooterWithDefaults } from "../lib/_footer-with-defaults";
import { SidebarWithDefaults } from "../lib/_sidebar-with-defaults";
import { BodyEndIslands } from "../lib/_body-end-islands";
import { composeMetaTitle } from "../lib/_compose-meta-title";
import { toSlugParams } from "@/utils/slug";
import { settings } from "@/config/settings";

// ---------------------------------------------------------------------------
// Props contract
// ---------------------------------------------------------------------------

interface DocPageProps {
  slug: string;
  title: string;
  description?: string;
  navSection?: string;
  hideSidebar?: boolean;
  hideToc?: boolean;
}

// ---------------------------------------------------------------------------
// Nav section detection
// ---------------------------------------------------------------------------

function detectNavSection(slug: string): string | undefined {
  // Look up which headerNav categoryMatch applies to this slug
  for (const item of settings.headerNav) {
    const match = (item as { categoryMatch?: string }).categoryMatch;
    if (match && (slug === match || slug.startsWith(match + "/"))) {
      return match;
    }
  }
  return undefined;
}

// ---------------------------------------------------------------------------
// paths() — synchronous route enumeration
// ---------------------------------------------------------------------------

export function paths(): Array<{
  params: { slug: string[] };
  props: DocPageProps & { params: { slug: string[] } };
}> {
  const entries = getDocs("docs");

  return entries
    .filter((entry) => {
      // Skip category_no_page entries — they label sidebar categories but
      // produce no page routes.
      if (entry.data.category_no_page) return false;
      // Skip draft entries
      if (entry.data.draft) return false;
      return true;
    })
    .map((entry) => {
      const slug = entry.id;
      const slugParams = toSlugParams(slug);
      const navSection = detectNavSection(slug);
      const props: DocPageProps & { params: { slug: string[] } } = {
        slug,
        title: entry.data.title,
        description: entry.data.description,
        navSection,
        hideSidebar: entry.data.hide_sidebar ?? false,
        hideToc: entry.data.hide_toc ?? false,
        params: { slug: slugParams },
      };
      return { params: { slug: slugParams }, props };
    });
}

// ---------------------------------------------------------------------------
// Page component
// ---------------------------------------------------------------------------

interface PageProps extends DocPageProps {
  params: { slug: string[] };
  // MDX render function injected by zfb
  Content?: (props: { components?: Record<string, unknown> }) => JSX.Element;
}

export default function DocsPage({
  slug,
  title,
  description,
  navSection,
  hideSidebar,
  hideToc,
  Content,
}: PageProps): JSX.Element {
  const metaTitle = composeMetaTitle(title);

  return (
    <DocLayoutWithDefaults
      title={metaTitle}
      lang="en"
      head={
        <HeadWithDefaults
          title={metaTitle}
          description={description}
        />
      }
      headerOverride={
        <HeaderWithDefaults
          lang="en"
          currentSlug={slug}
          navSection={navSection}
        />
      }
      sidebarOverride={
        hideSidebar ? undefined : (
          <SidebarWithDefaults
            currentSlug={slug}
            navSection={navSection}
          />
        )
      }
      hideSidebar={hideSidebar}
      hideToc={hideToc}
      footerOverride={<FooterWithDefaults />}
      bodyEndComponents={<BodyEndIslands />}
    >
      {Content ? (
        <Content components={mdxComponents as Record<string, unknown>} />
      ) : (
        <p>No content.</p>
      )}
    </DocLayoutWithDefaults>
  );
}
