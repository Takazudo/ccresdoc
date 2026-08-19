import { zudoDoc } from "@takazudo/zudo-doc/config";
import { defaultColorSchemes } from "@takazudo/zudo-doc/color-schemes-defaults";
import { defaultDirectiveVocabulary } from "@takazudo/zudo-doc/directive-vocabulary-defaults";
import { buildDocsSchema } from "@takazudo/zudo-doc/docs-schema";
import { defaultTranslations } from "@takazudo/zudo-doc/i18n-defaults";
import { defineConfig } from "zfb/config";

// zudoDoc still contributes Node-hosted plugins with package-owned routes
// disabled. CCResDoc owns routes and generation, so this final override is
// deliberately last and exactly empty.
export const config = defineConfig({
  ...zudoDoc({
    siteName: "CCResDoc",
    siteDescription: "Browse Claude Code resources from your local ~/.claude/",
    port: 4892,
    base: "/",
    trailingSlash: true,
    docsDir: "src/content/docs",
    defaultLocale: "en",
    locales: {},
    versions: false,
    onBrokenMarkdownLinks: "warn",
    sidebarResizer: true,
    sidebarToggle: true,
    dynamicPageTransition: true,
    findInPage: true,
    claudeResources: false,
    docHistory: false,
    llmsTxt: false,
    changelogs: false,
    packageOwnedRoutes: false,
    themePack: "default",
    buildDocsSchema,
    directives: defaultDirectiveVocabulary,
    translations: defaultTranslations,
    colorSchemes: defaultColorSchemes,
  }),
  plugins: [],
});

export default config;
