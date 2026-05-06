/**
 * Site footer — matches the DOM structure of the original footer.astro so
 * that Tailwind selectors in global.css work without modification.
 */

// currentYear is resolved at build time — not an environment variable.
const currentYear = new Date().getFullYear();

export default function Footer() {
  return (
    <footer class="border-t border-muted bg-surface">
      <div class="mx-auto max-w-[clamp(50rem,75vw,90rem)] px-hsp-xl py-vsp-xl lg:px-hsp-2xl lg:py-vsp-2xl">
        <div class="grid grid-cols-1 gap-vsp-lg sm:grid-cols-2">
          <div>
            <p class="text-small font-semibold text-fg mb-vsp-xs">Links</p>
            <ul class="list-none p-0">
              <li class="mb-vsp-2xs">
                <a
                  href="https://claude.com/claude-code"
                  class="text-caption text-muted hover:text-accent hover:underline"
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  Claude Code
                </a>
              </li>
              <li class="mb-vsp-2xs">
                <a
                  href="https://github.com/anthropics/claude-code"
                  class="text-caption text-muted hover:text-accent hover:underline"
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  GitHub
                </a>
              </li>
            </ul>
          </div>
        </div>
        <div class="mt-vsp-lg border-t border-muted pt-vsp-md text-center text-caption text-muted">
          Copyright &copy; {currentYear} CCResDoc.
        </div>
      </div>
    </footer>
  );
}
