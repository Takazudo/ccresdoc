import { zudoDoc } from "@takazudo/zudo-doc/config";
import { defaultColorSchemes } from "@takazudo/zudo-doc/color-schemes-defaults";
import { defaultDirectiveVocabulary } from "@takazudo/zudo-doc/directive-vocabulary-defaults";
import { buildDocsSchema } from "@takazudo/zudo-doc/docs-schema";
import { defaultTranslations } from "@takazudo/zudo-doc/i18n-defaults";
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
    buildDocsSchema,
    directives: defaultDirectiveVocabulary,
    translations: defaultTranslations,
    colorSchemes: defaultColorSchemes,
  }),
  plugins: [],
});

export default config;
