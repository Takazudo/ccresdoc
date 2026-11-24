export type Rules =
  | { kind: "claude_code" | "codex"; definition_version: number }
  | { kind: "custom"; definition_version: number; format: "markdown" | "code" | "skill"; include: string[]; exclude: string[] };
export type ScanRoot = { kind: "same" } | { kind: "separate"; path: string };
export type Preset = { id: string; name: string; description: string; version: number; rules: Rules };
export type Source = {
  id: string;
  label: string;
  enabled: boolean;
  root: string;
  scan_root: ScanRoot;
  mount: string;
  origin_preset: { id: string; name: string; version: number } | null;
  rules: Rules;
};
export type Catalog = { sources: Source[]; presets: Preset[] };

export const cloneCatalog = (catalog: Catalog): Catalog => structuredClone(catalog);
export const cloneRules = (rules: Rules): Rules => structuredClone(rules);

export function uniqueMount(catalog: Catalog, preferred: string): string {
  const used = new Set(catalog.sources.map((source) => source.mount));
  const base = preferred.toLowerCase().replace(/[^a-z0-9-]+/g, "-").replace(/^-+|-+$/g, "") || "documents";
  let mount = base;
  for (let index = 2; used.has(mount) || mount === "ccresdoc"; index++) mount = `${base}-${index}`;
  return mount;
}

export function sourceFromPreset(catalog: Catalog, preset: Preset, id: string): Source {
  const label = preset.name;
  return {
    id, label, enabled: false, root: "", scan_root: { kind: "same" },
    mount: uniqueMount(catalog, preset.id === "builtin-claude-code" ? "claude" : preset.id === "builtin-codex" ? "codex" : "documents"),
    origin_preset: { id: preset.id, name: preset.name, version: preset.version },
    rules: cloneRules(preset.rules),
  };
}

export function duplicateSource(catalog: Catalog, source: Source, id: string): Source {
  return { ...structuredClone(source), id, enabled: false, mount: uniqueMount(catalog, source.mount) };
}

export function applyPresetRules(source: Source, preset: Preset): Source {
  return {
    ...source,
    origin_preset: { id: preset.id, name: preset.name, version: preset.version },
    rules: cloneRules(preset.rules),
  };
}

export function saveRulesAsPreset(source: Source, id: string, name: string, description: string): { source: Source; preset: Preset } {
  const preset = { id, name, description, version: 1, rules: cloneRules(source.rules) };
  return { source: { ...source, origin_preset: { id, name, version: 1 } }, preset };
}

export function localCatalogErrors(catalog: Catalog): Array<{ field: string; message: string }> {
  const errors: Array<{ field: string; message: string }> = [];
  const mounts = new Set<string>();
  for (const source of catalog.sources) {
    if (!source.label.trim()) errors.push({ field: `sources.${source.id}.label`, message: "Enter a source name." });
    if (!/^[a-z0-9][a-z0-9._-]*$/.test(source.mount) || source.mount === "ccresdoc") errors.push({ field: `sources.${source.id}.mount`, message: "Use a unique lowercase URL name with letters, numbers, dots, underscores or hyphens." });
    if (mounts.has(source.mount)) errors.push({ field: `sources.${source.id}.mount`, message: "Another source already uses this URL name." });
    mounts.add(source.mount);
    if (source.enabled && !source.root.trim()) errors.push({ field: `sources.${source.id}.root`, message: "Choose a folder or disable this source." });
    if (source.enabled && source.rules.kind !== "custom" && source.scan_root.kind === "separate" && !source.scan_root.path.trim()) errors.push({ field: `sources.${source.id}.scan_root`, message: "Choose the instructions folder." });
    if (source.rules.kind === "custom" && source.rules.include.length === 0) errors.push({ field: `sources.${source.id}.rules.include`, message: "Enter at least one include pattern." });
  }
  return errors;
}
