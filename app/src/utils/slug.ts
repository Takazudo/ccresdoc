// Canonical route-slug helpers (mirrors zudo-doc's slug.ts).

export function toRouteSlug(id: string): string {
  // Both "index" (top-level) and "" (empty string from some generators) map
  // to the root route represented as "". A trailing "/index" segment is also
  // collapsed so "foo/index" → "foo" (fix #68).
  if (id === "index" || id === "") return "";
  return id.replace(/\/index$/, "");
}

export function toSlugParams(routeSlug: string): string[] {
  // Filter empty segments so that paths like "/a" or "a//b" do not produce
  // spurious empty strings in the params array (fix #68).
  return routeSlug === "" ? [] : routeSlug.split("/").filter(Boolean);
}

export function toTitleCase(str: string): string {
  return str
    .split("-")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}
