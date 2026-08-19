import manual from "../configs/manual.mjs";
import routesOff from "../configs/routes-off.mjs";
import selected from "../configs/selected.mjs";
import wholesale from "../configs/wholesale.mjs";

const summarize = (config) => ({
  framework: config.framework,
  port: config.port,
  base: config.base,
  trailingSlash: config.trailingSlash,
  minifyHtml: config.minifyHtml,
  collections: config.collections,
  plugins: config.plugins ?? [],
  markdown: config.markdown,
  codeHighlight: config.codeHighlight,
  resolveMarkdownLinks: config.resolveMarkdownLinks,
  stripMdExt: config.stripMdExt,
});

console.log(JSON.stringify({
  wholesale: summarize(wholesale),
  routesOff: summarize(routesOff),
  selected: summarize(selected),
  manual: summarize(manual),
}, null, 2));
