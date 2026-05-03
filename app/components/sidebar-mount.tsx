/**
 * Renders the sidebar container. The <nav> is intentionally empty at build
 * time — ccresdoc-sidebar.js fetches /api/manifest.json at runtime and
 * hydrates it with the category tree.
 *
 * The resizer handle sits at the right edge of the sidebar wrapper so it is
 * always in the correct position regardless of sidebar content.
 */
export default function SidebarMount() {
  return (
    <div class="ccresdoc-sidebar-wrapper">
      <nav id="ccresdoc-sidebar" aria-label="Site navigation">
        {/* Populated at runtime by ccresdoc-sidebar.js */}
      </nav>
      {/* Draggable resizer — JS in ccresdoc-sidebar.js attaches events */}
      <div class="ccresdoc-sidebar-resizer" id="ccresdoc-sidebar-resizer" aria-hidden="true" />
      <script src="/ccresdoc-sidebar.js" defer />
    </div>
  );
}
