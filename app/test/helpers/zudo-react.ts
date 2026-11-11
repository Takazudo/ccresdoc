import { h, flush, type Child, type Component, type Diagnostic } from "@takazudo/zfb/zudo-react";
import { renderToString } from "@takazudo/zfb/zudo-react/server";
import { createIslandTest, type IslandTest } from "@takazudo/zfb/zudo-react/testing";
import { mount } from "@takazudo/zfb/zudo-react/client";
import type { RootHandle } from "@takazudo/zfb/zudo-react/client";

export interface MountedView {
  readonly container: HTMLElement;
  readonly diagnostics: Diagnostic[];
  readonly root: RootHandle;
  unmount(): void;
}

const mounted = new Set<MountedView>();

/**
 * Client-mount a component with in-memory props (adapters, callbacks) that an
 * island's serialized props cannot carry. Diagnostics fail the mount loudly.
 */
export function mountView<P extends Record<string, unknown>>(
  component: Component<P>,
  props: P,
): MountedView {
  const name = (component as { displayName?: string }).displayName ?? component.name;
  // The client root validates the server-rendered island-host identity.
  const container = document.createElement("zfb-island");
  container.setAttribute("data-zfb-island-skip-ssr", name);
  container.setAttribute("data-zfb-transport", "json/1");
  container.setAttribute("data-zfb-protocol", "zudo-react/1");
  container.setAttribute("data-zfb-build", "test");
  document.body.append(container);
  const diagnostics: Diagnostic[] = [];
  const root = mount(h(component as Component<Record<string, unknown>>, props), container, {
    identity: { component: name, build: "test" },
    report: (diagnostic) => diagnostics.push(diagnostic),
  });
  if (!root) {
    container.remove();
    throw new Error(`mount failed: ${JSON.stringify(diagnostics)}`);
  }
  const view: MountedView = {
    container,
    diagnostics,
    root,
    unmount() {
      if (!mounted.delete(view)) return;
      root.unmount();
      container.remove();
    },
  };
  mounted.add(view);
  return view;
}

export function unmountAll(): void {
  for (const view of [...mounted]) view.unmount();
  disposeIslands();
}

/** Let queued signal updates and microtask continuations settle. */
export async function settle(): Promise<void> {
  await flush();
  await Promise.resolve();
  await flush();
}

export interface HydratedIsland {
  readonly host: Element;
  readonly island: IslandTest;
  dispose(): void;
}

const islands = new Set<IslandTest>();

/** Server-render one real island, hydrate it, and fail on any diagnostic. */
export async function hydrateIsland<P extends Record<string, unknown>>(
  component: Component<P>,
  props: P,
): Promise<HydratedIsland> {
  const island = createIslandTest(component, props, { document });
  islands.add(island);
  const root = island.hydrate();
  await island.flush();
  if (!root || island.diagnostics.length > 0) {
    throw new Error(`hydrate failed: ${JSON.stringify(island.diagnostics)}`);
  }
  return {
    host: island.host,
    island,
    dispose() {
      if (islands.delete(island)) island.dispose();
    },
  };
}

/** Static server markup parsed into a connected container. */
export function renderStatic(node: Child): HTMLElement {
  const container = document.createElement("div");
  container.innerHTML = renderToString(node);
  document.body.append(container);
  return container;
}

export function disposeIslands(): void {
  for (const island of [...islands]) {
    islands.delete(island);
    island.dispose();
  }
}
