import DefaultLayout from "../layouts/default";

export default function HomePage() {
  return (
    <DefaultLayout title="CCResDoc">
      <div class="ccresdoc-home-intro">
        <h1>CCResDoc</h1>
        <p>
          Browse Claude Code resources from your local <code>~/.claude/</code> — CLAUDE.md files,
          custom commands, skills, and agents.
        </p>
      </div>
      <ul class="ccresdoc-category-grid">
        <li>
          <a href="/claude-md/">CLAUDE.md</a>
        </li>
        <li>
          <a href="/claude-commands/">Commands</a>
        </li>
        <li>
          <a href="/claude-skills/">Skills</a>
        </li>
        <li>
          <a href="/claude-agents/">Agents</a>
        </li>
      </ul>
    </DefaultLayout>
  );
}
