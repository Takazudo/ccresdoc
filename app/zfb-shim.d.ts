// Local type shim for the bare `zfb/config` specifier.
//
// The zfb.config.ts imports from `zfb/config` (the bare specifier that zfb
// resolves at runtime to its config-stub). This ambient declaration provides
// the types so TypeScript can check the config file.
//
// Mirrors the zfb-shim.d.ts from the create-zudo-doc template — keep in sync
// with @takazudo/zfb/config exports when bumping the zfb version.

declare module "zfb/config" {
  export type Framework = "preact" | "react";

  export interface CollectionDef {
    name: string;
    path: string;
    schema?: Record<string, unknown>;
  }

  export interface TailwindConfig {
    enabled?: boolean;
  }

  export interface PluginConfig {
    name: string;
    options?: Record<string, unknown>;
  }

  export interface BundleConfig {
    exclude?: string[];
    mainFields?: string[];
    external?: string[];
  }

  // Named interface for the markdown feature keys used in zfb.config.ts.
  // Extend here when new features are added to the config.
  export interface MarkdownFeatures {
    directives?: Record<string, string>;
    mermaid?: boolean;
    headingMarkerToc?: boolean;
    githubAlerts?: boolean;
    readingTime?: boolean;
    codeEnrichment?: Record<string, unknown>;
    codeTabs?: boolean;
    ruby?: boolean;
    tocExport?: Record<string, unknown>;
    imageDimensions?: Record<string, unknown>;
    headingIds?: { strategy?: "flat" | "hierarchical" };
  }

  export interface ZfbConfig {
    outDir?: string;
    publicDir?: string;
    host?: string;
    port?: number;
    framework?: Framework;
    collections?: CollectionDef[];
    tailwind?: TailwindConfig;
    bundle?: BundleConfig;
    plugins?: PluginConfig[];
    adapter?: string;
    stripMdExt?: boolean;
    base?: string;
    codeHighlight?: {
      theme?: string;
      themesDir?: string;
    };
    resolveMarkdownLinks?: {
      enabled?: boolean;
      docsDir?: string;
      dirs?: Array<{ dir: string; routePrefix: string }>;
      onBrokenLinks?: "warn" | "error" | "ignore";
    };
    trailingSlash?: boolean;
    markdown?: {
      gfm?: boolean | Record<string, boolean>;
      toc?: Record<string, unknown>;
      externalLinks?: Record<string, unknown>;
      cjkFriendly?: boolean;
      features?: MarkdownFeatures;
    };
  }

  export function defineConfig(config: ZfbConfig): ZfbConfig;
}

declare module "zfb/content" {
  import type { FunctionComponent } from "preact";

  export interface CollectionEntry<T = unknown> {
    slug: string;
    body?: string;
    data: T;
    module_specifier: string;
    // Synchronous per zfb ADR-004 (getCollection is synchronous inside paths() evaluation)
    Content: FunctionComponent<{ components?: Record<string, unknown> }>;
  }

  // Synchronous: getCollection resolves from the pre-loaded ContentSnapshot.
  // Do NOT await — call sites in pages/_data.ts use it synchronously.
  export function getCollection<T = unknown>(name: string): CollectionEntry<T>[];
}
