// currentYear is resolved at build time — not an environment variable.
const currentYear = new Date().getFullYear();

export default function Footer() {
  return (
    <footer class="ccresdoc-footer">
      <div class="ccresdoc-footer-links">
        <h3>Links</h3>
        <a href="https://claude.com/claude-code" target="_blank" rel="noopener noreferrer">
          Claude Code
        </a>
        <a
          href="https://github.com/anthropics/claude-code"
          target="_blank"
          rel="noopener noreferrer"
        >
          GitHub
        </a>
      </div>
      <div class="ccresdoc-footer-copyright">
        Copyright &copy; {currentYear} CCResDoc.
      </div>
    </footer>
  );
}
