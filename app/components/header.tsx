import { Island } from "@takazudo/zfb";
import ThemeToggle from "./theme-toggle";

/**
 * Site header — matches the DOM structure of the original header.astro so
 * that Tailwind selectors in global.css work without modification.
 *
 * Search wiring is out of scope for S3; the slot placeholder is kept so
 * downstream waves can drop in a search component without touching this file.
 */
export default function Header() {
  return (
    <header class="sticky top-0 z-50 flex h-[3.5rem] items-center border-b border-muted bg-surface px-hsp-lg">
      <a
        class="whitespace-nowrap text-subheading font-bold text-fg hover:underline focus:underline"
        href="/"
      >
        CCResDoc
      </a>

      <div class="ml-auto flex items-center gap-x-hsp-md">
        {/* Search slot — wiring deferred to a later wave */}
        <Island when="idle">
          <ThemeToggle />
        </Island>
      </div>
    </header>
  );
}
