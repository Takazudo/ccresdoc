"use client";

// Use preact hook entrypoints directly — the "react" → "preact/compat" alias
// lets us consume React-typed components in this Preact app (configured
// project-wide).
import { useState, useCallback, useEffect, useMemo, useRef, useContext } from "preact/hooks";
import { createContext } from "preact";
import type { NavNode } from "@/utils/docs";
import type { LocaleLink } from "@/types/locale";
// Sidebar nav must react to the same client-router lifecycle event the mobile
// toggle (sidebar-toggle.tsx) listens on, so soft swaps re-sync the active slug.
import { AFTER_NAVIGATE_EVENT } from "@takazudo/zudo-doc/transitions";
import { INDENT, BASE_PAD, connectorLeft, ConnectorLines, CategoryLinkIcon } from "./tree-nav-shared";
import { stripBase } from "@/utils/base";
// BARE ThemeToggle (#2012 E2) — this footer toggle renders inside the
// SidebarToggle island, so it must NOT bring its own island wrapper.
import { ThemeToggle } from "@takazudo/zudo-doc/theme-toggle";
import { smartBreakToHtml } from "@/utils/smart-break";

function ToggleChevron({ isExpanded, className }: { isExpanded: boolean; className?: string }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      className={`h-[0.625rem] w-[0.625rem] shrink-0 transition-transform duration-150 ${isExpanded ? "rotate-90" : ""} ${className ?? ""}`}
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

const STORAGE_KEY = "zd-sidebar-open";

function padLeft(depth: number, forCategory: boolean): string {
  if (depth === 0) return `calc(${BASE_PAD} + ${forCategory ? "0.15rem" : "0rem"})`;
  return `calc(${depth} * ${INDENT} + 1.25rem + 5px)`;
}

function getOpenSet(): Set<string> {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return new Set();
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? new Set(parsed.filter((v): v is string => typeof v === "string")) : new Set();
  } catch {
    return new Set();
  }
}

/**
 * Whether a persisted open-set exists at all. Distinguishes "fresh session,
 * no saved state" (keep server defaults) from "user has a saved set"
 * (the set is the post-hydration source of truth for every category).
 */
function hasStoredOpenSet(): boolean {
  try {
    return sessionStorage.getItem(STORAGE_KEY) !== null;
  } catch {
    return false;
  }
}

function saveOpenSet(set: Set<string>) {
  try {
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify([...set]));
  } catch {
    // ignore
  }
}

function normalizePath(p: string): string {
  return p.replace(/\/$/, "") || "/";
}

// --- Tree keyboard navigation (WAI-ARIA tree pattern) ---------------------
//
// Implements roving tabindex + arrow-key navigation across the visible
// treeitems. Only one row is tabbable at a time (`tabindex=0`); the rest are
// `tabindex=-1` and reached via Up/Down. Right/Left expand/collapse (or move
// to parent/first child). Movement is computed by querying the live DOM for
// `[role="treeitem"]` inside the tree root — collapsed children are not
// rendered, so the query naturally yields exactly the visible rows in order.

interface CategoryOpenControls {
  setOpen: (v: boolean) => void;
}

interface TreeNavContextValue {
  rootRef: { current: HTMLDivElement | null };
  /** Update the roving-tabindex target (lives as state in SidebarTree). */
  setFocusedId: (id: string) => void;
  /** Toggle a category open/closed by slug (used by Right/Left/Space). */
  setOpenBySlug: (slug: string, open: boolean) => void;
  /** Registry of per-category open controls, keyed by slug (filled by CategoryNode). */
  _setters: { current: Map<string, CategoryOpenControls> };
}

const TreeNavContext = createContext<TreeNavContextValue | null>(null);

function visibleItems(root: HTMLElement | null): HTMLElement[] {
  if (!root) return [];
  return Array.from(root.querySelectorAll<HTMLElement>('[role="treeitem"]'));
}

function focusItem(el: HTMLElement | undefined, setFocusedId: (id: string) => void) {
  if (!el) return;
  const id = el.getAttribute("data-tree-id");
  if (id !== null) setFocusedId(id);
  el.focus();
}

/**
 * Keydown handler for a single treeitem row. `ctx` carries the shared tree
 * state; `slug`, `isExpandable`, `expanded`, and `level` describe this row.
 * Returns nothing — it mutates focus / open-state as a side effect.
 */
function handleTreeKeyDown(
  e: KeyboardEvent,
  ctx: TreeNavContextValue,
  row: HTMLElement,
  slug: string,
  isExpandable: boolean,
  expanded: boolean,
  level: number,
) {
  const root = ctx.rootRef.current;
  const items = visibleItems(root);
  const idx = items.indexOf(row);
  if (idx === -1) return;

  switch (e.key) {
    case "ArrowDown": {
      e.preventDefault();
      focusItem(items[idx + 1], ctx.setFocusedId);
      break;
    }
    case "ArrowUp": {
      e.preventDefault();
      focusItem(items[idx - 1], ctx.setFocusedId);
      break;
    }
    case "ArrowRight": {
      e.preventDefault();
      if (isExpandable && !expanded) {
        ctx.setOpenBySlug(slug, true);
      } else if (isExpandable && expanded) {
        // Move to first child (next row, which will be one level deeper).
        const next = items[idx + 1];
        if (next && Number(next.getAttribute("aria-level")) > level) {
          focusItem(next, ctx.setFocusedId);
        }
      }
      break;
    }
    case "ArrowLeft": {
      e.preventDefault();
      if (isExpandable && expanded) {
        ctx.setOpenBySlug(slug, false);
      } else {
        // Move focus to the parent row (nearest preceding shallower level).
        for (let i = idx - 1; i >= 0; i--) {
          if (Number(items[i].getAttribute("aria-level")) < level) {
            focusItem(items[i], ctx.setFocusedId);
            break;
          }
        }
      }
      break;
    }
    case "Home": {
      e.preventDefault();
      focusItem(items[0], ctx.setFocusedId);
      break;
    }
    case "End": {
      e.preventDefault();
      focusItem(items[items.length - 1], ctx.setFocusedId);
      break;
    }
    case " ": {
      // Space toggles expansion on an expandable row (WAI-ARIA tree pattern);
      // on a leaf it activates the link. The inner anchor/button are
      // tabindex=-1, so forward the activation here.
      e.preventDefault();
      if (isExpandable) {
        ctx.setOpenBySlug(slug, !expanded);
      } else {
        row.querySelector<HTMLElement>("a[href]")?.click();
      }
      break;
    }
    case "Enter": {
      // Enter activates the row's link if it has one; otherwise (a no-href
      // category) it toggles expansion.
      e.preventDefault();
      const link = row.querySelector<HTMLElement>("a[href]");
      if (link) {
        link.click();
      } else if (isExpandable) {
        ctx.setOpenBySlug(slug, !expanded);
      }
      break;
    }
    default:
      break;
  }
}

/**
 * Single-sources the `role="treeitem"` row contract shared by CategoryNode
 * and LeafNode: roving tabindex, aria-level/selected, the data-tree-id used by
 * focus movement, and the keydown handler. `aria-expanded` is emitted only for
 * expandable rows (leaves must not carry it). Returns the props to spread onto
 * the row `<div>` plus the context (CategoryNode also needs it for the
 * open-control registry).
 *
 * NOTE: `data-tree-id` / roving focus assume `slug` is unique across the WHOLE
 * tree (not per-parent). buildNavTree guarantees this today; if that ever
 * changes, focus movement would jump to the first slug-duplicate row.
 */
function useTreeItem(opts: {
  slug: string;
  level: number;
  isActive: boolean;
  isTabbable: boolean;
  isExpandable: boolean;
  expanded: boolean;
}) {
  const treeNav = useContext(TreeNavContext);
  const rowRef = useRef<HTMLDivElement | null>(null);

  const onKeyDown = (e: KeyboardEvent) => {
    if (!treeNav || !rowRef.current) return;
    handleTreeKeyDown(e, treeNav, rowRef.current, opts.slug, opts.isExpandable, opts.expanded, opts.level);
  };

  const rowProps = {
    ref: rowRef,
    role: "treeitem" as const,
    "aria-level": opts.level,
    "aria-selected": opts.isActive,
    ...(opts.isExpandable ? { "aria-expanded": opts.expanded } : {}),
    "data-tree-id": opts.slug,
    tabIndex: opts.isTabbable ? 0 : -1,
    onKeyDown,
    className: "relative outline-none focus-visible:ring-1 focus-visible:ring-accent",
  };

  return { treeNav, rowProps };
}

/** Find the slug of the node whose href matches the given pathname */
function findActiveSlug(nodes: NavNode[], pathname: string): string | undefined {
  // Strip the base prefix from pathname so that node.href values (which are
  // stored without the base) compare correctly under a non-root deployment
  // (fix: active highlight was broken when base !== "/").
  const pathnameWithoutBase = normalizePath(stripBase(pathname));
  for (const node of nodes) {
    if (node.href && normalizePath(node.href) === pathnameWithoutBase) return node.slug;
    const found = findActiveSlug(node.children, pathname);
    // "" is the canonical root-index slug (#1891) — a truthiness check
    // would discard a legitimate root match.
    if (found !== undefined) return found;
  }
  return undefined;
}

/** Track current active slug, updating on View Transition navigations */
function useActiveSlug(nodes: NavNode[], initial?: string): string | undefined {
  const [slug, setSlug] = useState(initial);

  useEffect(() => {
    const update = () => {
      const pathname = normalizePath(window.location.pathname);
      const found = findActiveSlug(nodes, pathname);
      if (found !== undefined) setSlug(found);
    };
    // Initial run on mount, then re-sync after every client-router soft swap.
    // Using AFTER_NAVIGATE_EVENT (the same signal sidebar-toggle.tsx listens
    // on) keeps the active slug correct across soft navigations, where
    // `DOMContentLoaded` never fires again.
    update();
    document.addEventListener(AFTER_NAVIGATE_EVENT, update);
    return () => document.removeEventListener(AFTER_NAVIGATE_EVENT, update);
  }, [nodes]);

  return slug;
}

function filterTree(nodes: NavNode[], query: string): NavNode[] {
  return nodes.reduce<NavNode[]>((acc, node) => {
    const matchesLabel = node.label.toLowerCase().includes(query.toLowerCase());
    const filteredChildren = node.children.length > 0
      ? filterTree(node.children, query)
      : [];

    if (matchesLabel || filteredChildren.length > 0) {
      acc.push({
        ...node,
        children: matchesLabel ? node.children : filteredChildren,
      });
    }
    return acc;
  }, []);
}

interface RootMenuItem {
  label: string;
  href: string;
  children?: RootMenuItem[];
}

function RootMenuItemEntry({ item }: { item: RootMenuItem }) {
  const [expanded, setExpanded] = useState(false);
  const hasChildren = item.children && item.children.length > 0;

  return (
    <div className="border-t border-muted">
      <div className="flex items-center">
        <a
          href={item.href}
          className="flex flex-1 items-center gap-hsp-xs px-hsp-sm py-vsp-xs text-small font-semibold text-fg hover:text-accent hover:underline break-words"
        >
          <CategoryLinkIcon className="w-[14px]" />
          <span dangerouslySetInnerHTML={{ __html: smartBreakToHtml(item.label) }} />
        </a>
        {hasChildren && (
          <button
            type="button"
            onClick={() => setExpanded((prev) => !prev)}
            className="flex items-center justify-center px-hsp-sm py-vsp-xs text-muted hover:text-fg"
            aria-expanded={expanded}
            aria-label={expanded ? `Collapse ${item.label}` : `Expand ${item.label}`}
          >
            <ToggleChevron isExpanded={expanded} className="text-muted" />
          </button>
        )}
      </div>
      {hasChildren && expanded && (
        <div className="pb-vsp-xs">
          {item.children!.map((child) => (
            <a
              key={child.href}
              href={child.href}
              className="block pl-hsp-xl pr-hsp-sm py-vsp-2xs text-small text-muted hover:text-accent hover:underline break-words"
            >
              <span dangerouslySetInnerHTML={{ __html: smartBreakToHtml(child.label) }} />
            </a>
          ))}
        </div>
      )}
    </div>
  );
}

interface SidebarTreeProps {
  nodes: NavNode[];
  currentSlug?: string;
  rootMenuItems?: RootMenuItem[];
  backToMenuLabel?: string;
  localeLinks?: LocaleLink[];
  themeDefaultMode?: "light" | "dark";
}

function SidebarFooter({ links, themeDefaultMode }: { links?: LocaleLink[]; themeDefaultMode?: "light" | "dark" }) {
  if (!links && !themeDefaultMode) return null;
  return (
    // pb-[50vh] provides scroll room so the footer doesn't sit at the very bottom of the viewport
    <div className="lg:hidden flex items-center gap-hsp-md border-t border-muted px-hsp-sm py-vsp-xs pb-[50vh] text-small">
      {themeDefaultMode && <ThemeToggle defaultMode={themeDefaultMode} />}
      {links && links.map((link, i) => (
        <span key={link.href} className="flex items-center gap-hsp-xs">
          {i > 0 && <span className="text-muted">/</span>}
          {link.active ? (
            <span aria-current="true" className="font-medium text-fg">{link.label}</span>
          ) : (
            <a href={link.href} lang={link.code} className="text-muted hover:text-fg">
              {link.label}
            </a>
          )}
        </span>
      ))}
    </div>
  );
}

/**
 * Builds a STABLE tree-navigation context value plus the roving-tabindex
 * pointer. `focusedId` is deliberately NOT part of the context object — every
 * arrow keystroke updates it, and folding it into the Provider value would
 * re-render every consuming row on each keypress. Instead it stays as plain
 * state here and feeds the `tabbableId` derivation; the context only carries
 * stable refs/callbacks (the slug→setOpen registry that lets keyboard
 * Right/Left/Space drive the same per-node open state the toggle button drives).
 */
function useTreeNav(): { ctx: TreeNavContextValue; focusedId: string | null } {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [focusedId, setFocusedIdState] = useState<string | null>(null);
  const settersRef = useRef(new Map<string, CategoryOpenControls>());

  const setFocusedId = useCallback((id: string) => setFocusedIdState(id), []);
  const setOpenBySlug = useCallback((slug: string, open: boolean) => {
    settersRef.current.get(slug)?.setOpen(open);
  }, []);

  const ctx = useMemo<TreeNavContextValue>(
    () => ({ rootRef, setFocusedId, setOpenBySlug, _setters: settersRef }),
    [setFocusedId, setOpenBySlug],
  );

  return { ctx, focusedId };
}

export default function SidebarTree({ nodes, currentSlug, rootMenuItems, backToMenuLabel, localeLinks, themeDefaultMode }: SidebarTreeProps) {
  const activeSlug = useActiveSlug(nodes, currentSlug);
  const { ctx: treeNavCtx, focusedId } = useTreeNav();
  const [query, setQuery] = useState("");
  const [showingRootMenu, setShowingRootMenu] = useState(false);
  const filterRef = useRef<HTMLInputElement>(null);
  const [filterPlaceholder, setFilterPlaceholder] = useState("Filter...");

  // Detect OS to show appropriate keyboard shortcut in placeholder
  useEffect(() => {
    const platform = (navigator as { userAgentData?: { platform: string } }).userAgentData?.platform ?? navigator.platform;
    const isMac = /mac/i.test(platform);
    setFilterPlaceholder(isMac ? "Filter... (\u2318 + /)" : "Filter... (Ctrl + /)");
  }, []);

  // Global shortcut: Cmd+/ (Mac) or Ctrl+/ to focus the filter input
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.isComposing) return;
      if (e.key === "/" && (e.metaKey || e.ctrlKey)) {
        const el = filterRef.current;
        if (!el || el.offsetParent === null) return; // skip if hidden
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

  // Roving-tabindex target: exactly one treeitem is tabbable (`tabindex=0`).
  // Until the user moves focus (focusedId === null) the active page's row is
  // the entry point; failing that, the first top-level row. Once the user
  // arrows around, focusedId pins the tabbable row.
  const firstVisibleSlug = filteredNodes[0]?.slug;
  const tabbableId = focusedId ?? activeSlug ?? firstVisibleSlug;

  // Guard against zero tabbable rows: `tabbableId` is a slug, but a row only
  // renders as a treeitem when its ancestors are expanded. If the active page
  // (or a stored focusedId) sits inside a collapsed subtree, NO rendered row
  // matches and Tab can't enter the tree. After each render, if nothing is
  // tabbable, pin focus to the first actually-rendered treeitem.
  useEffect(() => {
    const root = treeNavCtx.rootRef.current;
    if (!root) return;
    const items = visibleItems(root);
    if (items.length === 0) return;
    const hasTabbable = items.some((el) => el.getAttribute("data-tree-id") === tabbableId);
    if (!hasTabbable) {
      const firstId = items[0].getAttribute("data-tree-id");
      if (firstId !== null) treeNavCtx.setFocusedId(firstId);
    }
  });

  const footer = useMemo(
    () => (localeLinks || themeDefaultMode) ? <SidebarFooter links={localeLinks} themeDefaultMode={themeDefaultMode} /> : null,
    [localeLinks, themeDefaultMode],
  );

  // Root menu view: show headerNav items as a simple list (Docusaurus-style)
  if (showingRootMenu && rootMenuItems) {
    return (
      <nav>
        <button
          type="button"
          onClick={() => setShowingRootMenu(false)}
          className="flex w-full items-center gap-hsp-xs px-hsp-sm py-vsp-xs text-left text-small text-muted hover:text-fg border-b border-muted"
        >
          <svg className="h-icon-sm w-icon-sm shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M9 5l7 7-7 7" />
          </svg>
          {backToMenuLabel ?? "Back to main menu"}
        </button>
        {rootMenuItems.map((item) => (
          <RootMenuItemEntry key={item.href} item={item} />
        ))}
        {footer}
      </nav>
    );
  }

  // Top page: show only header nav links, no doc tree or filter.
  // Derived from activeSlug (runtime-synced) so it stays correct across View
  // Transitions. Must be an undefined check, not truthiness: "" is the
  // canonical root-index doc slug (#1891) and gets the full tree.
  if (activeSlug === undefined && rootMenuItems) {
    return (
      <nav>
        {rootMenuItems.map((item) => (
          <RootMenuItemEntry key={item.href} item={item} />
        ))}
        {footer}
      </nav>
    );
  }

  return (
    <nav>
      {rootMenuItems && (
        <button
          type="button"
          onClick={() => setShowingRootMenu(true)}
          className="lg:hidden flex w-full items-center gap-hsp-xs px-hsp-sm py-vsp-xs text-left text-small text-muted hover:text-fg border-b border-muted"
        >
          <svg className="h-icon-sm w-icon-sm shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M15 19l-7-7 7-7" />
          </svg>
          {backToMenuLabel ?? "Back to main menu"}
        </button>
      )}
      <div className="px-hsp-sm py-vsp-xs">
        <div className="flex items-center gap-hsp-xs bg-surface rounded px-hsp-sm py-vsp-2xs">
          <svg className="h-[14px] w-[14px] text-muted shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
          <input
            ref={filterRef}
            type="text"
            aria-label="Filter sidebar"
            placeholder={filterPlaceholder}
            value={query}
            // onInput (not onChange): under zfb's esbuild Preact the filter
            // input must fire per-keystroke. Preact binds onChange to the native
            // `change` event (fires on blur) — so onChange makes the filter
            // blur-only. onInput fires on every keystroke for a live filter.
            // (The zudo-doc scaffold uses onChange, which only works on its
            // preact/compat build where onChange === onInput; see upstream report.)
            onInput={(e) => setQuery(e.currentTarget.value)}
            className="bg-transparent text-small outline-none w-full text-fg placeholder:text-muted"
          />
        </div>
      </div>
      <TreeNavContext.Provider value={treeNavCtx}>
        <div ref={treeNavCtx.rootRef} role="tree" aria-label="Documentation navigation">
          <NodeList
            nodes={filteredNodes}
            currentSlug={activeSlug}
            depth={0}
            forceOpen={!!query}
            tabbableId={tabbableId}
          />
        </div>
      </TreeNavContext.Provider>
      {footer}
    </nav>
  );
}

function NodeList({
  nodes,
  currentSlug,
  depth,
  forceOpen,
  tabbableId,
}: {
  nodes: NavNode[];
  currentSlug?: string;
  depth: number;
  forceOpen: boolean;
  /** Slug of the single roving-tabindex target across the whole tree. */
  tabbableId?: string;
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
            tabbableId={tabbableId}
          />
        ) : (
          <LeafNode
            key={node.slug}
            node={node}
            currentSlug={currentSlug}
            depth={depth}
            isLast={isLast}
            tabbableId={tabbableId}
          />
        );
      })}
    </>
  );
}

/** Check if currentSlug is anywhere in this node's subtree */
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
  tabbableId,
}: {
  node: NavNode;
  currentSlug?: string;
  depth: number;
  isLast: boolean;
  forceOpen: boolean;
  tabbableId?: string;
}) {
  const containsCurrent = subtreeContainsSlug(node, currentSlug);
  const isActive = node.slug === currentSlug;

  // Initial state must match server render (no sessionStorage access)
  // to avoid hydration mismatch. Stored state is restored in useEffect below.
  const [open, setOpen] = useState(containsCurrent ? true : !node.collapsed);

  // --- Tree keyboard nav wiring -------------------------------------------
  const isExpanded = forceOpen || open;
  const level = depth + 1; // WAI-ARIA aria-level is 1-based
  const { treeNav, rowProps } = useTreeItem({
    slug: node.slug,
    level,
    isActive,
    isTabbable: tabbableId === node.slug,
    isExpandable: true,
    expanded: isExpanded,
  });

  // Persist-aware open setter shared by the toggle button and keyboard
  // Right/Left/Space, so every open/close path writes the sessionStorage set
  // consistently (fix #4 made the stored set the source of truth).
  const setOpenPersisted = useCallback((next: boolean) => {
    setOpen(next);
    const stored = getOpenSet();
    if (next) stored.add(node.slug);
    else stored.delete(node.slug);
    saveOpenSet(stored);
  }, [node.slug]);

  // Register this category's open control so keyboard Right/Left/Space (routed
  // at the root by slug) drives the same per-node state the toggle button does.
  useEffect(() => {
    if (!treeNav) return;
    const setters = treeNav._setters;
    setters.current.set(node.slug, { setOpen: setOpenPersisted });
    return () => {
      setters.current.delete(node.slug);
    };
  }, [treeNav, node.slug, setOpenPersisted]);

  // Restore open state from sessionStorage after hydration. The stored set is
  // the post-hydration source of truth and reconciles BOTH directions: a slug
  // absent from the set closes, a slug present opens. We only override the
  // server default when a saved set actually exists (fresh sessions keep
  // server defaults). The active subtree always stays open regardless, so a
  // stale "closed" entry can't hide the current page.
  useEffect(() => {
    if (!hasStoredOpenSet()) return;
    const stored = getOpenSet();
    const next = containsCurrent || stored.has(node.slug);
    if (next !== open) {
      setOpen(next);
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

  // Sync auto-opened state to sessionStorage so it persists across View Transitions
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
    setOpenPersisted(!open);
  }, [setOpenPersisted, open]);

  const paddingLeft = padLeft(depth, true);

  return (
    <div className={`${depth === 0 ? "border-t border-muted" : ""} ${depth >= 1 && !isLast ? "relative" : ""}`}>
      {depth >= 1 && !isLast && isExpanded && (
        <div
          className="absolute border-l border-solid border-muted z-10"
          style={{
            left: connectorLeft(depth),
            top: 0,
            bottom: 0,
          }}
        />
      )}
      <div {...rowProps}>
        <ConnectorLines depth={depth} isLast={isLast} topPad="calc(0.15rem + var(--spacing-vsp-xs))" />
        {node.href ? (
          <div
            className={`flex w-full items-center text-small font-semibold pt-[0.15rem] ${isActive ? "bg-fg text-bg" : "text-fg"}`}
          >
            <a
              href={node.href}
              tabIndex={-1}
              aria-current={isActive ? "page" : undefined}
              className={`flex-1 flex items-start gap-hsp-xs py-vsp-xs hover:underline focus:underline break-words ${isActive ? "text-bg" : "text-fg"}`}
              style={{ paddingLeft }}
            >
              {depth === 0 && (
                <span className="flex h-[1lh] items-center">
                  <CategoryLinkIcon className={`w-[14px] ${isActive ? "text-bg" : ""}`} />
                </span>
              )}
              <span dangerouslySetInnerHTML={{ __html: smartBreakToHtml(node.label) }} />
            </a>
            <button
              type="button"
              tabIndex={-1}
              onClick={toggle}
              className={`aspect-square flex items-center justify-center w-[1.5rem] border-y border-l hover:underline focus:underline ${isActive ? "border-bg/30" : "border-muted"}`}
              aria-expanded={isExpanded}
              aria-label={isExpanded ? `Collapse ${node.label}` : `Expand ${node.label}`}
            >
              <ToggleChevron isExpanded={isExpanded} className={isActive ? "text-bg" : "text-muted"} />
            </button>
          </div>
        ) : (
          <button
            type="button"
            tabIndex={-1}
            onClick={toggle}
            className={`flex w-full items-center gap-hsp-md text-left text-small font-semibold py-vsp-xs text-fg hover:underline focus:underline break-words`}
            style={{ paddingLeft }}
            aria-expanded={isExpanded}
            aria-label={isExpanded ? `Collapse ${node.label}` : `Expand ${node.label}`}
          >
            <span className="aspect-square flex items-center justify-center w-[1.5rem] shrink-0 border border-muted">
              <ToggleChevron isExpanded={isExpanded} className="text-muted" />
            </span>
            <span dangerouslySetInnerHTML={{ __html: smartBreakToHtml(node.label) }} />
          </button>
        )}
      </div>
      {isExpanded && (
        <div role="group">
          <NodeList
            nodes={node.children}
            currentSlug={currentSlug}
            depth={depth + 1}
            forceOpen={forceOpen}
            tabbableId={tabbableId}
          />
        </div>
      )}
    </div>
  );
}

function LeafNode({
  node,
  currentSlug,
  depth,
  isLast,
  tabbableId,
}: {
  node: NavNode;
  currentSlug?: string;
  depth: number;
  isLast: boolean;
  tabbableId?: string;
}) {
  const isActive = node.slug === currentSlug;
  const level = depth + 1; // WAI-ARIA aria-level is 1-based
  // Leaves are not expandable, so Right/Left only move focus.
  const { rowProps } = useTreeItem({
    slug: node.slug,
    level,
    isActive,
    isTabbable: tabbableId === node.slug,
    isExpandable: false,
    expanded: false,
  });

  // Hook calls above must run before this early return so hook order stays
  // stable across renders.
  if (!node.href) return null;
  const isRoot = depth === 0;
  const paddingLeft = padLeft(depth, isRoot);

  // For nested last leaves, add visual breathing space as margin on the outer wrapper
  // rather than padding on the anchor — padding would grow the row box and throw off
  // the ConnectorLines geometry (which now uses topPad + 0.5lh of the row to land the
  // horizontal connector at the first-line midpoint).
  const outerClass = isRoot
    ? "border-t border-muted"
    : !isRoot && isLast
      ? "pb-vsp-md"
      : "";

  const topPad = isRoot
    ? "calc(var(--spacing-vsp-xs) + 0.15rem)"
    : "var(--spacing-vsp-2xs)";

  return (
    <div className={outerClass}>
      <div {...rowProps}>
        <ConnectorLines depth={depth} isLast={isLast} topPad={topPad} />
        <a
          href={node.href}
          tabIndex={-1}
          aria-current={isActive ? "page" : undefined}
          className={isRoot
            ? `flex items-start gap-hsp-xs py-[calc(var(--spacing-vsp-xs)+0.15rem)] pr-[4px] text-small font-semibold break-words ${
                isActive ? "bg-fg text-bg" : "text-fg hover:underline focus:underline"
              }`
            : `block py-vsp-2xs pr-[4px] text-small break-words ${
                isActive
                  ? "bg-fg font-medium text-bg"
                  : "text-muted hover:underline focus:underline"
              }`
          }
          style={{ paddingLeft }}
        >
          {isRoot && (
            <span className="flex h-[1lh] items-center">
              <CategoryLinkIcon className={`w-[14px] ${isActive ? "text-bg" : ""}`} />
            </span>
          )}
          <span dangerouslySetInnerHTML={{ __html: smartBreakToHtml(node.label) }} />
        </a>
      </div>
    </div>
  );
}
