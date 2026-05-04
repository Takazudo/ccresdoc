/**
 * DocMetainfo — page metadata display (created/updated dates, author).
 * Ported from $HOME/.claude/doc/src/components/doc-metainfo.astro.
 *
 * In the original, git info is fetched server-side via getGitInfo().
 * Here it is passed in as props since this component renders inside the
 * axum-rendered shell.
 */

export interface DocMetainfoData {
  createdAt?: string;
  updatedAt?: string;
  author?: string;
}

interface Props {
  info: DocMetainfoData;
  locale?: string;
}

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  } catch {
    return iso;
  }
}

export function DocMetainfo({ info, locale: _locale = "en" }: Props) {
  const hasInfo = info.createdAt || info.updatedAt;
  if (!hasInfo) return null;

  return (
    <div class="flex flex-wrap items-center gap-x-hsp-md gap-y-vsp-2xs text-caption text-fg mb-vsp-md border-t border-fg pt-vsp-xs">
      {info.createdAt && (
        <span class="inline-flex items-center gap-hsp-2xs">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            class="h-[0.75rem] w-[0.75rem]"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            stroke-width="2"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M12 6v6l4 2m6-2a9 9 0 11-18 0 9 9 0 0118 0z"
            />
          </svg>
          <span>Created {formatDate(info.createdAt)}</span>
        </span>
      )}
      {info.updatedAt && info.updatedAt !== info.createdAt && (
        <span class="inline-flex items-center gap-hsp-2xs">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            class="h-[0.75rem] w-[0.75rem]"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            stroke-width="2"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
            />
          </svg>
          <span>Updated {formatDate(info.updatedAt)}</span>
        </span>
      )}
      {info.author && (
        <span class="inline-flex items-center gap-hsp-2xs">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            class="h-[0.75rem] w-[0.75rem]"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            stroke-width="2"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z"
            />
          </svg>
          <span>{info.author}</span>
        </span>
      )}
    </div>
  );
}
