import { createMdxComponents } from "@takazudo/zudo-doc/mdx-components";
import { ProbeCounter } from "@/components/probe-counter";

const EmptyNav = () => null;

export const mdxComponents = createMdxComponents({
  settings: {
    base: "/",
    imageEnlarge: false,
    assetViewerDir: "assets",
    assetViewerRoutePrefix: "files",
  },
  locale: "en",
  currentSlug: "probe",
  navData: {
    CategoryNav: EmptyNav,
    CategoryTreeNav: EmptyNav,
    SiteTreeNav: EmptyNav,
    NoteTrayIndex: EmptyNav,
  },
  extras: { ProbeCounter },
});
