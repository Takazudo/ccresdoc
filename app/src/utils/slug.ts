// Canonical route-slug helpers (mirrors zudo-doc's slug.ts).

export function toRouteSlug(id: string): string {
  if (id === "index") return "";
  return id.replace(/\/index$/, "");
}

export function toSlugParams(routeSlug: string): string[] {
  return routeSlug === "" ? [] : routeSlug.split("/");
}

export function toTitleCase(str: string): string {
  return str
    .split("-")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
}
