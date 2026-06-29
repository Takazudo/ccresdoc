/** @jsxRuntime automatic */
/** @jsxImportSource preact */
// Body-end islands for CCResDoc.
//
// Minimal set: ClientRouterBootstrap (enables SPA soft-swap navigation) plus
// CodeBlockEnhancer (copy + word-wrap buttons on code blocks) plus the optional
// SidebarResizerInit drag handle (gated on settings.sidebarResizer).
// No AI chat modal, no design token panel, no image enlarge.

import type { JSX, VNode } from "preact";
import { Island } from "@takazudo/zfb";
// SidebarResizerInit is a plain inline <script> (not a hydrated island): it
// attaches a drag handle to #desktop-sidebar on load and after each route swap.
// Render it directly — no Island wrapper — mirroring zudo-doc's scaffold.
import { SidebarResizerInit } from "@takazudo/zudo-doc/sidebar-resizer";
// CodeBlockEnhancer is a plain inline <script> (not a hydrated island): it wraps
// <pre class="syntect-*"> elements in .code-block-wrapper and adds copy +
// word-wrap buttons, wiring zfb navigation events for SPA re-init.
// Render it directly — no Island wrapper — same pattern as SidebarResizerInit.
import { CodeBlockEnhancer } from "@takazudo/zudo-doc/code-syntax";
import ClientRouterBootstrap from "@/components/client-router-bootstrap";
import { settings } from "@/config/settings";

(ClientRouterBootstrap as { displayName?: string }).displayName =
  "ClientRouterBootstrap";

export function BodyEndIslands(): JSX.Element {
  const routerIsland = Island({
    when: "load",
    children: <ClientRouterBootstrap />,
  }) as unknown as VNode;

  return (
    <>
      {routerIsland}
      <CodeBlockEnhancer />
      {settings.sidebarResizer && <SidebarResizerInit />}
    </>
  );
}
