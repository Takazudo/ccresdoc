import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { checkInstalled, checkScope, checkScopesAgree } from "./lib/dependency-contract.mjs";

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const roots = {
  app: join(repoRoot, "app"),
  probe: join(repoRoot, "compatibility", "node-free-latest"),
};
const args = process.argv.slice(2);
const installed = args.includes("--installed");
const scopeArg = args.includes("--scope") ? args[args.indexOf("--scope") + 1] : "all";
const scopes = scopeArg === "all" ? Object.keys(roots) : [scopeArg];
if (!scopes.every((scope) => Object.hasOwn(roots, scope))) {
  console.error(`unknown --scope ${scopeArg}; expected app, probe or all`);
  process.exit(2);
}

const read = (root, file) => readFileSync(join(root, file), "utf8");
const manifests = {};
const errors = [];
for (const scope of scopes) {
  const root = roots[scope];
  manifests[scope] = JSON.parse(read(root, "package.json"));
  errors.push(...checkScope(scope, {
    packageJson: manifests[scope],
    lockfile: read(root, "pnpm-lock.yaml"),
    workspace: read(root, "pnpm-workspace.yaml"),
    readPatch: (path) => (existsSync(join(root, path)) ? readFileSync(join(root, path)) : null),
  }));
  if (installed) errors.push(...checkInstalled(scope, root));
}
if (manifests.app && manifests.probe) errors.push(...checkScopesAgree(manifests.app, manifests.probe));

if (errors.length > 0) {
  console.error("dependency validation failed:");
  for (const message of errors) console.error(`- ${message}`);
  process.exit(1);
}
console.log(`dependency validation passed (${scopes.join(", ")}: manifest, workspace, lockfile, peers${installed ? ", installed tree" : ""})`);
