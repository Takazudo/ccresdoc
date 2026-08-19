import { createMdxComponents } from "@takazudo/zudo-doc/mdx-components";
import { ProbeCounter } from "@/components/probe-counter";

const EmptyNav = () => null;

export const mdxComponents = createMdxComponents({
  settings: { base: "/", imageEnlarge: false },
  locale: "en",
  navData: {
    CategoryNav: EmptyNav,
    CategoryTreeNav: EmptyNav,
    SiteTreeNav: EmptyNav,
  },
  extras: { ProbeCounter },
});
