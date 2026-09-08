const escapeRegExp = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

function pathAliases(path, platform) {
  const aliases = [path];
  // macOS exposes /tmp and /var through symlinks into /private. Native
  // diagnostics and esbuild can disagree about which spelling to print.
  if (platform === "darwin") {
    if (/^\/private\/(?:tmp|var)(?:\/|$)/.test(path)) aliases.push(path.slice("/private".length));
    else if (/^\/(?:tmp|var)(?:\/|$)/.test(path)) aliases.push(`/private${path}`);
  }
  return aliases;
}

export function createProbeOutputNormalizer({ probeRoot, tempRoot, platform }) {
  const probePaths = pathAliases(probeRoot, platform).map(escapeRegExp);
  const relativeProbePaths = pathAliases(probeRoot, platform)
    .map((path) => escapeRegExp(path.replace(/^\/+/, "")));
  const tempPaths = pathAliases(tempRoot.replace(/\/$/, ""), platform).map(escapeRegExp);
  const relativeProbePattern = new RegExp(`(?:\\.\\.\\/)+(?:${relativeProbePaths.join("|")})(?=/|$)`, "g");
  const probePattern = new RegExp(`(?<![\\w./\\\\-])(?:${probePaths.join("|")})(?=[/\\\\]|$)`, "g");
  const workspacePattern = new RegExp(`(?:${tempPaths.join("|")})/ccresdoc-config-[^/ ]+/workspace(?=/|$)`, "g");
  const pluginHostPattern = new RegExp(`(?:${tempPaths.join("|")})/zfb-plugin-host-[^/ ]+`, "g");
  return (value) => value
    .replace(relativeProbePattern, "<probe-root>")
    .replace(probePattern, "<probe-root>")
    .replace(workspacePattern, "<isolated-workspace>")
    .replace(pluginHostPattern, "<zfb-plugin-host>")
    .replace(/ in \d+\.\d+s/g, " in <duration>");
}
