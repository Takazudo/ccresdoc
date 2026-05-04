import { defineConfig } from "@takazudo/zfb/config";

export default defineConfig({
  framework: "preact",
  tailwind: {
    enabled: true,
  },
  // No SSR adapter — pure static build; axum serves the static output at runtime.
  plugins: [
    {
      // Workaround: zfb does not copy public/ to dist/ in production builds.
      // This plugin fills the gap. Remove when zfb adds native public-dir copy.
      name: "./plugins/copy-public.mjs",
    },
  ],
});
