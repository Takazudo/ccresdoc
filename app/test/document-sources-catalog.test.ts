import { describe, expect, it } from "vitest";
import { applyPresetRules, duplicateSource, localCatalogErrors, saveRulesAsPreset, sourceFromPreset, type Catalog, type Preset } from "../src/document-sources/catalog";

const markdown: Preset = { id: "builtin-markdown", name: "Markdown", description: "", version: 1, rules: { kind: "custom", definition_version: 1, format: "markdown", include: ["**/*.md"], exclude: [] } };

describe("source and preset drafts", () => {
  it("makes independent snapshots for two sources using one preset", () => {
    const catalog: Catalog = { sources: [], presets: [] };
    const first = sourceFromPreset(catalog, markdown, "one");
    catalog.sources.push(first);
    const second = sourceFromPreset(catalog, markdown, "two");
    expect(second.mount).toBe("documents-2");
    if (first.rules.kind !== "custom" || second.rules.kind !== "custom") throw new Error("expected custom rules");
    first.rules.include.push("**/*.mdx");
    expect(second.rules.include).toEqual(["**/*.md"]);
    markdown.rules = { kind: "custom", definition_version: 2, format: "code", include: ["**/*.ts"], exclude: [] };
    expect(second.rules.format).toBe("markdown");
    const reapplied = applyPresetRules(second, markdown);
    expect(reapplied.rules).toEqual(markdown.rules);
    expect(reapplied.root).toBe(second.root);
    expect(reapplied.mount).toBe(second.mount);
  });

  it("duplicates disabled with a new ID and mount", () => {
    const catalog: Catalog = { sources: [], presets: [] };
    const original = sourceFromPreset(catalog, markdown, "one");
    catalog.sources.push(original);
    const copy = duplicateSource(catalog, original, "two");
    expect(copy).toMatchObject({ id: "two", enabled: false, mount: "documents-2" });
    expect(copy.root).toBe(original.root);
  });

  it("saves rules as a preset without changing paths or rule bytes", () => {
    const source = sourceFromPreset({ sources: [], presets: [] }, markdown, "one");
    source.root = "/tmp/example";
    const { source: linked, preset } = saveRulesAsPreset(source, "mine", "My rules", "desc");
    expect(linked.root).toBe(source.root);
    expect(linked.rules).toEqual(source.rules);
    expect(linked.origin_preset?.id).toBe("mine");
    expect(preset).not.toHaveProperty("root");
  });

  it("does not require a hidden scan root for custom rules", () => {
    const source = sourceFromPreset({ sources: [], presets: [] }, markdown, "one");
    source.enabled = true;
    source.root = "/tmp/documents";
    source.scan_root = { kind: "separate", path: "" };
    expect(localCatalogErrors({ sources: [source], presets: [] })).toEqual([]);
  });
});
