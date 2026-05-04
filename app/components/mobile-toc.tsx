"use client";

/**
 * MobileToc — collapsed mobile drawer for table of contents.
 * Ported from $HOME/.claude/doc/src/components/mobile-toc.tsx.
 * Hidden on xl+ breakpoints (desktop Toc takes over there).
 */

import { useState, useMemo } from "preact/hooks";
import type { Heading } from "./toc";

interface MobileTocProps {
  headings: Heading[];
  title?: string;
}

export function MobileToc({ headings, title = "On this page" }: MobileTocProps) {
  const filtered = useMemo(
    () => headings.filter((h) => h.depth >= 2 && h.depth <= 4),
    [headings],
  );
  const [open, setOpen] = useState(false);

  if (filtered.length === 0) return <div class="hidden" />;

  return (
    <div class="xl:hidden border border-muted mb-vsp-lg">
      <button
        type="button"
        onClick={() => setOpen((prev) => !prev)}
        aria-expanded={open}
        class="flex w-full items-center justify-between px-hsp-lg py-vsp-xs text-small font-medium text-fg"
      >
        <span>{title}</span>
        <svg
          xmlns="http://www.w3.org/2000/svg"
          aria-hidden="true"
          class={`h-[1rem] w-[1rem] text-muted transition-transform duration-150${open ? " rotate-180" : ""}`}
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          stroke-width={2}
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M19 9l-7 7-7-7"
          />
        </svg>
      </button>
      {open && (
        <ul class="border-t border-muted px-hsp-lg py-vsp-xs space-y-vsp-2xs">
          {filtered.map((heading, index) => {
            const depthClass =
              heading.depth === 3
                ? "ml-hsp-lg"
                : heading.depth === 4
                  ? "ml-hsp-2xl"
                  : "";
            return (
              <li key={`${heading.slug}-${index}`} class={depthClass}>
                <a
                  href={`#${heading.slug}`}
                  onClick={() => setOpen(false)}
                  class="block py-vsp-2xs text-small text-muted hover:text-fg hover:underline"
                >
                  {heading.text}
                </a>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
