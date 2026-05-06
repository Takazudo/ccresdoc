"use client";

import { useState, useEffect } from "preact/hooks";
import SidebarTree, { type NavNode } from "./sidebar-tree";

// ---------------------------------------------------------------------------
// Manifest types — mirror the shape produced by ccresdoc-server/manifest.rs
// ---------------------------------------------------------------------------

interface ManifestItem {
  slug: string;
  label: string;
  path: string;
}

interface ManifestCategory {
  slug: string;
  label: string;
  items: ManifestItem[];
}

interface Manifest {
  generatedAt: string;
  categories: ManifestCategory[];
}

// ---------------------------------------------------------------------------
// Convert manifest → NavNode tree
//
// Each ManifestCategory becomes a root-level NavNode (no href, with children).
// Each ManifestItem becomes a leaf NavNode.
// ---------------------------------------------------------------------------

function manifestToNavNodes(manifest: Manifest): NavNode[] {
  return manifest.categories.map((cat) => ({
    slug: cat.slug,
    label: cat.label,
    href: undefined,
    children: cat.items.map((item) => ({
      slug: item.slug,
      label: item.label,
      href: item.path,
      children: [],
    })),
  }));
}

// ---------------------------------------------------------------------------
// Sidebar — fetches manifest at hydration, renders tree
//
// Sidebar data delivery: fetch from the existing /api/manifest.json endpoint
// at hydration time. This avoids modifying the Rust server to inject a new
// sentinel into the shell HTML (zfb does not have a data-injection point for
// shell templates, so the sentinel approach would require server changes).
// ---------------------------------------------------------------------------

export default function Sidebar() {
  const [nodes, setNodes] = useState<NavNode[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetch("/api/manifest.json")
      .then((res) => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        return res.json() as Promise<Manifest>;
      })
      .then((manifest) => {
        setNodes(manifestToNavNodes(manifest));
      })
      .catch((err: unknown) => {
        const msg = err instanceof Error ? err.message : String(err);
        setError(msg);
      });
  }, []);

  if (error) {
    return (
      <div class="px-hsp-sm py-vsp-xs text-caption text-muted">
        Failed to load navigation: {error}
      </div>
    );
  }

  if (nodes.length === 0) {
    return (
      <div class="px-hsp-sm py-vsp-xs text-caption text-muted">Loading…</div>
    );
  }

  return <SidebarTree nodes={nodes} />;
}
