"use client";

/**
 * Toc — right-rail table of contents for desktop.
 * Ported from $HOME/.claude/doc/src/components/toc.tsx.
 * Uses useActiveHeading to highlight the in-viewport heading.
 */

import { useMemo, useEffect, useState } from "preact/hooks";

export interface Heading {
  depth: number;
  slug: string;
  text: string;
}

interface TocProps {
  headings: Heading[];
}

/**
 * Tracks which heading is currently in the viewport using IntersectionObserver.
 * Returns the slug of the active heading.
 */
function useActiveHeading(headings: Heading[]): string | null {
  const [activeId, setActiveId] = useState<string | null>(null);

  useEffect(() => {
    if (headings.length === 0) return;

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            setActiveId(entry.target.id);
          }
        }
      },
      {
        rootMargin: "0px 0px -80% 0px",
        threshold: 0,
      },
    );

    for (const heading of headings) {
      const el = document.getElementById(heading.slug);
      if (el) observer.observe(el);
    }

    return () => observer.disconnect();
  }, [headings]);

  return activeId;
}

export function Toc({ headings }: TocProps) {
  const filtered = useMemo(
    () => headings.filter((h) => h.depth >= 2 && h.depth <= 4),
    [headings],
  );
  const activeId = useActiveHeading(filtered);

  if (filtered.length === 0) return <nav class="hidden" />;

  return (
    <nav
      aria-label="Table of contents"
      class="hidden xl:block w-[280px] shrink-0 sticky top-[3.5rem] self-start z-10 pt-vsp-xl lg:pt-vsp-2xl max-h-[calc(100vh-3.5rem)] overflow-y-auto"
    >
      <ul class="border-l border-muted pl-hsp-lg">
        {filtered.map((heading, index) => {
          const isActive = heading.slug === activeId;
          const depthClass =
            heading.depth === 3
              ? "ml-hsp-lg"
              : heading.depth === 4
                ? "ml-hsp-2xl"
                : "";
          const linkClass = isActive
            ? "block py-vsp-2xs text-small leading-snug transition-colors bg-fg text-bg font-medium"
            : "block py-vsp-2xs text-small leading-snug transition-colors text-muted hover:underline focus:underline";
          return (
            <li key={`${heading.slug}-${index}`} class={depthClass}>
              <a
                href={`#${heading.slug}`}
                aria-current={isActive ? "true" : undefined}
                class={linkClass}
              >
                {heading.text}
              </a>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
