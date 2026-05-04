"use client";

import { useState, useCallback, useEffect, useMemo, useRef } from "preact/hooks";
import {
  INDENT,
  BASE_PAD,
  connectorLeft,
  ConnectorLines,
  CategoryLinkIcon,
} from "./tree-nav-shared";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface NavNode {
  slug: string;
  label: string;
  href?: string;
  children: NavNode[];
}

// ---------------------------------------------------------------------------
// Session storage helpers for open/closed state
// ---------------------------------------------------------------------------

// Key matches the original sidebar-tree.tsx so existing stored state is preserved.
const STORAGE_KEY = "zd-sidebar-open";

function getOpenSet(): Set<string> {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return new Set();
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed)
      ? new Set(parsed.filter((v): v is string => typeof v === "string"))
      : new Set();
  } catch {
    return new Set();
  }
}

function saveOpenSet(set: Set<string>) {
  try {
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify([...set]));
  } catch {
    // ignore
  }
}

// ---------------------------------------------------------------------------
// Chevron icon
// ---------------------------------------------------------------------------

function ToggleChevron({
  isExpanded,
  className,
}: {
  isExpanded: boolean;
  className?: string;
}) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      class={`h-[0.625rem] w-[0.625rem] shrink-0 transition-transform duration-150 ${
        isExpanded ? "rotate-90" : ""
      } ${className ?? ""}`}
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden="true"
    >
      <path strokeLinecap="round" strokeLinejoin="round" d="M9 5l7 7-7 7" />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Padding helpers
// ---------------------------------------------------------------------------

function padLeft(depth: number, forCategory: boolean): string {
  if (depth === 0) return `calc(${BASE_PAD} + ${forCategory ? "0.15rem" : "0rem"})`;
  return `calc(${depth} * ${INDENT} + 1.25rem + 5px)`;
}

// ---------------------------------------------------------------------------
// Active slug tracking
// ---------------------------------------------------------------------------

function normalizePath(p: string): string {
  return p.replace(/\/$/, "") || "/";
}

function findActiveSlug(nodes: NavNode[], pathname: string): string | undefined {
  for (const node of nodes) {
    if (node.href && normalizePath(node.href) === pathname) return node.slug;
    const found = findActiveSlug(node.children, pathname);
    if (found) return found;
  }
  return undefined;
}

function useActiveSlug(nodes: NavNode[], initial?: string): string | undefined {
  const [slug, setSlug] = useState(initial);

  useEffect(() => {
    const update = () => {
      const pathname = normalizePath(window.location.pathname);
      const found = findActiveSlug(nodes, pathname);
      if (found !== undefined) setSlug(found);
    };
    update();
  }, [nodes]);

  return slug;
}

// ---------------------------------------------------------------------------
// Tree filtering
// ---------------------------------------------------------------------------

function filterTree(nodes: NavNode[], query: string): NavNode[] {
  return nodes.reduce<NavNode[]>((acc, node) => {
    const matchesLabel = node.label.toLowerCase().includes(query.toLowerCase());
    const filteredChildren =
      node.children.length > 0 ? filterTree(node.children, query) : [];

    if (matchesLabel || filteredChildren.length > 0) {
      acc.push({
        ...node,
        children: matchesLabel ? node.children : filteredChildren,
      });
    }
    return acc;
  }, []);
}

// ---------------------------------------------------------------------------
// SidebarTree — root component
// ---------------------------------------------------------------------------

interface SidebarTreeProps {
  nodes: NavNode[];
  currentSlug?: string;
}

export default function SidebarTree({ nodes, currentSlug }: SidebarTreeProps) {
  const activeSlug = useActiveSlug(nodes, currentSlug);
  const [query, setQuery] = useState("");
  const filterRef = useRef<HTMLInputElement>(null);
  const [filterPlaceholder, setFilterPlaceholder] = useState("Filter...");

  // Detect OS to show appropriate keyboard shortcut in placeholder
  useEffect(() => {
    const platform =
      (navigator as { userAgentData?: { platform: string } }).userAgentData
        ?.platform ?? navigator.platform;
    const isMac = /mac/i.test(platform);
    setFilterPlaceholder(isMac ? "Filter... (⌘ + /)" : "Filter... (Ctrl + /)");
  }, []);

  // Global shortcut: Cmd+/ (Mac) or Ctrl+/ to focus the filter input
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.isComposing) return;
      if (e.key === "/" && (e.metaKey || e.ctrlKey)) {
        const el = filterRef.current;
        if (!el || el.offsetParent === null) return;
        e.preventDefault();
        el.focus();
        el.select();
      }
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, []);

  const filteredNodes = useMemo(
    () => (query ? filterTree(nodes, query) : nodes),
    [nodes, query],
  );

  return (
    <nav>
      <div class="px-hsp-sm py-vsp-xs">
        <div class="flex items-center gap-hsp-xs bg-surface rounded px-hsp-sm py-vsp-2xs">
          <svg
            class="h-[14px] w-[14px] text-muted shrink-0"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            strokeWidth={2}
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
            />
          </svg>
          <input
            ref={filterRef}
            type="text"
            placeholder={filterPlaceholder}
            value={query}
            onInput={(e) => setQuery((e.target as HTMLInputElement).value)}
            class="bg-transparent text-small outline-none w-full text-fg placeholder:text-muted"
          />
        </div>
      </div>
      <NodeList
        nodes={filteredNodes}
        currentSlug={activeSlug}
        depth={0}
        forceOpen={!!query}
      />
    </nav>
  );
}

// ---------------------------------------------------------------------------
// NodeList
// ---------------------------------------------------------------------------

function NodeList({
  nodes,
  currentSlug,
  depth,
  forceOpen,
}: {
  nodes: NavNode[];
  currentSlug?: string;
  depth: number;
  forceOpen: boolean;
}) {
  return (
    <>
      {nodes.map((node, index) => {
        const isLast = index === nodes.length - 1;
        return node.children.length > 0 ? (
          <CategoryNode
            key={node.slug}
            node={node}
            currentSlug={currentSlug}
            depth={depth}
            isLast={isLast}
            forceOpen={forceOpen}
          />
        ) : (
          <LeafNode
            key={node.slug}
            node={node}
            currentSlug={currentSlug}
            depth={depth}
            isLast={isLast}
          />
        );
      })}
    </>
  );
}

// ---------------------------------------------------------------------------
// CategoryNode
// ---------------------------------------------------------------------------

function subtreeContainsSlug(node: NavNode, slug?: string): boolean {
  if (!slug) return false;
  if (node.slug === slug) return true;
  return node.children.some((child) => subtreeContainsSlug(child, slug));
}

function CategoryNode({
  node,
  currentSlug,
  depth,
  isLast,
  forceOpen,
}: {
  node: NavNode;
  currentSlug?: string;
  depth: number;
  isLast: boolean;
  forceOpen: boolean;
}) {
  const containsCurrent = subtreeContainsSlug(node, currentSlug);
  const isActive = node.slug === currentSlug;

  const [open, setOpen] = useState(containsCurrent);

  // Restore open state from sessionStorage after hydration
  useEffect(() => {
    const stored = getOpenSet();
    if (stored.has(node.slug) && !open) {
      setOpen(true);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Auto-open category when navigation lands on a descendant
  useEffect(() => {
    if (subtreeContainsSlug(node, currentSlug) && !open) {
      setOpen(true);
      const stored = getOpenSet();
      stored.add(node.slug);
      saveOpenSet(stored);
    }
  }, [currentSlug]); // eslint-disable-line react-hooks/exhaustive-deps

  // Sync auto-opened state to sessionStorage
  useEffect(() => {
    if (open) {
      const stored = getOpenSet();
      if (!stored.has(node.slug)) {
        stored.add(node.slug);
        saveOpenSet(stored);
      }
    }
  }, [open, node.slug]);

  const toggle = useCallback(() => {
    setOpen((prev) => {
      const next = !prev;
      const stored = getOpenSet();
      if (next) {
        stored.add(node.slug);
      } else {
        stored.delete(node.slug);
      }
      saveOpenSet(stored);
      return next;
    });
  }, [node.slug]);

  const isExpanded = forceOpen || open;
  const paddingLeft = padLeft(depth, true);

  return (
    <div
      class={`${depth === 0 ? "border-t border-muted" : ""} ${
        depth >= 1 && !isLast ? "relative" : ""
      }`}
    >
      {depth >= 1 && !isLast && isExpanded && (
        <div
          class="absolute border-l border-solid border-muted z-10"
          style={{
            left: connectorLeft(depth),
            top: 0,
            bottom: 0,
          }}
        />
      )}
      <div class="relative">
        <ConnectorLines depth={depth} isLast={isLast} />
        {node.href ? (
          <div
            class={`flex w-full items-center text-small font-semibold pt-[0.15rem] ${
              isActive ? "bg-fg text-bg" : "text-fg"
            }`}
          >
            <a
              href={node.href}
              aria-current={isActive ? "page" : undefined}
              class={`flex-1 flex items-center gap-hsp-xs py-vsp-xs hover:underline focus:underline ${
                isActive ? "text-bg" : "text-fg"
              }`}
              style={{ paddingLeft }}
            >
              {depth === 0 && (
                <CategoryLinkIcon class={`w-[14px] ${isActive ? "text-bg" : ""}`} />
              )}
              {node.label}
            </a>
            <button
              type="button"
              onClick={toggle}
              class={`aspect-square flex items-center justify-center w-[1.5rem] border-y border-l hover:underline focus:underline ${
                isActive ? "border-bg/30" : "border-muted"
              }`}
              aria-expanded={isExpanded}
              aria-label={isExpanded ? `Collapse ${node.label}` : `Expand ${node.label}`}
            >
              <ToggleChevron
                isExpanded={isExpanded}
                className={isActive ? "text-bg" : "text-muted"}
              />
            </button>
          </div>
        ) : (
          <button
            type="button"
            onClick={toggle}
            class="flex w-full items-center gap-hsp-md text-small font-semibold py-vsp-xs text-fg hover:underline focus:underline"
            style={{ paddingLeft }}
            aria-expanded={isExpanded}
            aria-label={isExpanded ? `Collapse ${node.label}` : `Expand ${node.label}`}
          >
            <span class="aspect-square flex items-center justify-center w-[1.5rem] shrink-0 border border-muted">
              <ToggleChevron isExpanded={isExpanded} className="text-muted" />
            </span>
            {node.label}
          </button>
        )}
      </div>
      {isExpanded && (
        <div>
          <NodeList
            nodes={node.children}
            currentSlug={currentSlug}
            depth={depth + 1}
            forceOpen={forceOpen}
          />
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// LeafNode
// ---------------------------------------------------------------------------

function LeafNode({
  node,
  currentSlug,
  depth,
  isLast,
}: {
  node: NavNode;
  currentSlug?: string;
  depth: number;
  isLast: boolean;
}) {
  if (!node.href) return null;
  const isActive = node.slug === currentSlug;
  const isRoot = depth === 0;
  const paddingLeft = padLeft(depth, isRoot);

  return (
    <div class={isRoot ? "border-t border-muted" : ""}>
      <div class="relative">
        <ConnectorLines depth={depth} isLast={isLast} />
        <a
          href={node.href}
          class={
            isRoot
              ? `flex items-center gap-hsp-xs py-[calc(var(--spacing-vsp-xs)+0.15rem)] pr-[4px] text-small font-semibold ${
                  isActive ? "bg-fg text-bg" : "text-fg hover:underline focus:underline"
                }`
              : `block py-vsp-2xs pr-[4px] ${isLast ? "pb-vsp-xs" : ""} text-small ${
                  isActive
                    ? "bg-fg font-medium text-bg"
                    : "text-muted hover:underline focus:underline"
                }`
          }
          style={{ paddingLeft }}
        >
          {isRoot && (
            <CategoryLinkIcon class={`w-[14px] ${isActive ? "text-bg" : ""}`} />
          )}
          {node.label}
        </a>
      </div>
    </div>
  );
}
