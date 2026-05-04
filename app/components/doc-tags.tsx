/**
 * DocTags — tag list shown below the page content.
 * Ported from $HOME/.claude/doc/src/components/doc-tags.astro.
 */

interface Props {
  tags: string[];
  basePath?: string;
}

export function DocTags({ tags, basePath = "/docs/tags" }: Props) {
  if (tags.length === 0) return null;

  return (
    <div class="mt-vsp-md mb-vsp-lg flex flex-wrap items-center gap-hsp-xs">
      <span class="text-caption text-muted">Tags:</span>
      {tags.map((tag) => (
        <a
          key={tag}
          href={`${basePath}/${tag}`}
          class="inline-block px-hsp-sm py-vsp-2xs text-caption bg-surface border border-muted rounded-full text-accent hover:bg-code-bg hover:border-accent"
        >
          {tag}
        </a>
      ))}
    </div>
  );
}
