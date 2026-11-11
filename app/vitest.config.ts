import { defineConfig } from "vitest/config";
import { fileURLToPath } from "node:url";

export default defineConfig({
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
      // zfb resolves this bare specifier itself; Vite needs the package path.
      "zfb/config": "@takazudo/zfb/config",
    },
  },
  oxc: {
    jsx: {
      runtime: "automatic",
      importSource: "@takazudo/zfb/zudo-react",
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
        // Route tests mock @takazudo/zfb/content; zudo-doc must go through
        // Vite for that mock to reach the package's own content import.
        inline: [/@takazudo\/zudo-doc/],
      },
    },
  },
});
