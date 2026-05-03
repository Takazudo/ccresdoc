import DefaultLayout from "../layouts/default";

/**
 * Shell page — produces dist/shell/index.html which the copy-public postBuild
 * plugin then moves to dist/_shell/index.html. This is the runtime substitution
 * template used by the axum server (S5).
 *
 * Note: the file is named shell.tsx (not _shell.tsx) because zfb's router
 * skips files whose stem starts with '_'. The postBuild plugin in
 * plugins/copy-public.mjs handles the rename to dist/_shell/.
 *
 * The axum server loads dist/_shell/index.html and string-replaces:
 *   ☃CCRESDOC_TITLE_SLOT☃   → HTML-escaped page title (inside <title>)
 *   ☃CCRESDOC_CONTENT_SLOT☃ → rendered HTML fragment (inside <main>)
 *
 * CRITICAL: the sentinel literals must appear verbatim in the built HTML.
 * Do NOT escape or transform them — dangerouslySetInnerHTML is used so
 * JSX does not entity-encode the snowman characters.
 */
export default function ShellPage() {
  return (
    <DefaultLayout title={"☃CCRESDOC_TITLE_SLOT☃"}>
      <span dangerouslySetInnerHTML={{ __html: "☃CCRESDOC_CONTENT_SLOT☃" }} />
    </DefaultLayout>
  );
}
