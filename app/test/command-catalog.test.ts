import { describe, expect, it } from "vitest";
import catalog from "../src/browser-chrome/command-catalog.json";

describe("browser command catalog", () => {
  it("exposes the versioned neutral command contract", () => {
    expect(catalog.version).toBe(1);
    expect(
      catalog.commands.map(({ commandId, label, defaultBindings }) => ({
        commandId,
        label,
        defaultBindings,
      })),
    ).toEqual([
      { commandId: "back", label: "Back", defaultBindings: ["Mod+["] },
      {
        commandId: "forward",
        label: "Forward",
        defaultBindings: ["Mod+]"],
      },
      { commandId: "home", label: "Home", defaultBindings: [] },
      {
        commandId: "reload-documentation",
        label: "Reload Documentation",
        defaultBindings: ["Mod+R"],
      },
      {
        commandId: "find-in-page",
        label: "Find in Page",
        defaultBindings: ["Mod+F"],
      },
      {
        commandId: "search-documentation",
        label: "Search Documentation",
        defaultBindings: ["Mod+K"],
      },
      {
        commandId: "copy-page-path",
        label: "Copy Page Path",
        defaultBindings: [],
      },
      {
        commandId: "open-in-default-browser",
        label: "Open in Default Browser",
        defaultBindings: [],
      },
    ]);
    expect(new Set(catalog.commands.map((command) => command.commandId)).size).toBe(
      catalog.commands.length,
    );
    for (const command of catalog.commands) {
      expect(command.group).not.toBe("");
      expect(command.menu.name).not.toBe("");
      expect(command.menu.order).toBeGreaterThan(0);
      for (const binding of command.defaultBindings) {
        expect(binding).toMatch(/^(?:Mod|Ctrl|Alt|Shift)\+/);
        expect(binding).not.toMatch(/Cmd|Command|Meta/);
      }
    }
  });
});
