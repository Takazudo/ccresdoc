/** @jsxRuntime automatic */
/** @jsxImportSource preact */
// Body-end islands for CCResDoc.
//
// Minimal set: ClientRouterBootstrap (enables SPA soft-swap navigation).
// No AI chat modal, no design token panel, no image enlarge.

import type { JSX, VNode } from "preact";
import { Island } from "@takazudo/zfb";
import ClientRouterBootstrap from "@/components/client-router-bootstrap";

(ClientRouterBootstrap as { displayName?: string }).displayName =
  "ClientRouterBootstrap";

export function BodyEndIslands(): JSX.Element {
  const routerIsland = Island({
    when: "load",
    children: <ClientRouterBootstrap />,
  }) as unknown as VNode;

  return <>{routerIsland}</>;
}
