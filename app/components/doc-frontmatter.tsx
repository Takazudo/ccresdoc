/**
 * DocFrontmatter — frontmatter key-value display.
 * Ported from $HOME/.claude/doc/src/components/doc-frontmatter.astro.
 */

import { Fragment } from "preact";

export interface FrontmatterEntry {
  key: string;
  value: string | number | boolean | string[];
}

interface Props {
  entries: FrontmatterEntry[];
}

export function DocFrontmatter({ entries }: Props) {
  const visibleEntries = entries.filter(
    (e) => e.value !== undefined && e.value !== null && e.value !== "",
  );
  if (visibleEntries.length === 0) return null;

  return (
    <div class="mb-vsp-md border border-muted rounded bg-surface px-hsp-lg py-vsp-xs">
      <dl class="grid grid-cols-[auto_1fr] gap-x-hsp-lg gap-y-vsp-2xs text-small">
        {visibleEntries.map((entry) => (
          <Fragment key={entry.key}>
            <dt class="font-mono text-caption text-muted whitespace-nowrap">
              {entry.key}
            </dt>
            <dd class="text-fg break-words">
              {Array.isArray(entry.value)
                ? entry.value.join(", ")
                : String(entry.value)}
            </dd>
          </Fragment>
        ))}
      </dl>
    </div>
  );
}
