import { defineConfig } from "@takazudo/zfb/config";

export default defineConfig({
  framework: "preact",
  tailwind: {
    enabled: true,
  },
  // No SSR adapter — pure static build; axum serves the static output at runtime.
  plugins: [
    {
      // Workaround: zfb router skips underscore-prefixed pages; rename dist/shell/ → dist/_shell/.
      // Remove when zfb supports an opt-in escape hatch for underscore-prefixed pages.
      name: "./plugins/rename-shell.mjs",
    },
  ],
});
