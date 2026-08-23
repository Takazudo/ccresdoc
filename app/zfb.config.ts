import { zudoDoc } from "@takazudo/zudo-doc/config";
import { defineConfig } from "zfb/config";
import { settings } from "./src/config/settings";

// zudoDoc still contributes Node-hosted plugins with package-owned routes
// disabled. CCResDoc owns routes and generation, so this final override is
// deliberately last and exactly empty.
export const config = defineConfig({
  ...zudoDoc({
    ...settings,
    port: 4892,
    onBrokenMarkdownLinks: "warn",
  }),
  plugins: [],
});

export default config;
