import { defineConfig } from "vitest/config";
import { fileURLToPath } from "node:url";

export default defineConfig({
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
      "zfb/config": "@takazudo/zfb/config",
      "react/jsx-runtime": "preact/jsx-runtime",
      "react/jsx-dev-runtime": "preact/jsx-runtime",
      "react-dom/test-utils": "preact/test-utils",
      "react-dom": "preact/compat",
      react: "preact/compat",
    },
  },
  test: {
    environment: "happy-dom",
    setupFiles: ["./test/setup.ts"],
    include: ["test/**/*.test.{ts,tsx}"],
    clearMocks: true,
    restoreMocks: true,
    server: {
      deps: {
        // The published zfb runtime imports React-compatible entrypoints.
        // Inline it so Vite applies the Preact aliases above.
        inline: [
          /@takazudo\/zudo-doc/,
          /@takazudo\/zfb-runtime/,
          /@takazudo\/zfb(?:\/|$)/,
        ],
      },
    },
  },
});
