// Audited published-toolchain tuple for the app and the compatibility probe.
// These values are written by hand after registry review; validators compare
// manifests, lockfiles and installed trees against them, never against
// values read back from the manifests themselves.

export const ZFB_VERSION = "7.2.0";
export const ZUDO_DOC_VERSION = "14.0.0";
export const PACKAGE_MANAGER = "pnpm@10.30.3";
export const NODE_ENGINE = ">=24.0.0";
export const PNPM_ENGINE = ">=10.0.0";
export const MINIMUM_RELEASE_AGE = 1440;

export const NATIVE_CARRIERS = Object.freeze({
  "darwin-arm64": "@takazudo/zfb-darwin-arm64",
  "darwin-x64": "@takazudo/zfb-darwin-x64",
  "linux-arm64": "@takazudo/zfb-linux-arm64-gnu",
  "linux-x64": "@takazudo/zfb-linux-x64-gnu",
  "win32-x64": "@takazudo/zfb-win32-x64-msvc",
});

export const RUNTIME_DEPENDENCIES = Object.freeze({
  "@takazudo/zfb": ZFB_VERSION,
  "@takazudo/zfb-md-wasm": ZFB_VERSION,
  "@takazudo/zfb-runtime": ZFB_VERSION,
  "@takazudo/zudo-doc": ZUDO_DOC_VERSION,
  katex: "0.19.1",
  zod: "4.6.7",
});

export const CARRIER_DEPENDENCIES = Object.freeze(
  Object.fromEntries(Object.values(NATIVE_CARRIERS).map((name) => [name, ZFB_VERSION])),
);

export const APP_DEV_DEPENDENCIES = Object.freeze({
  "happy-dom": "21.0.4",
  playwright: "1.62.1",
  typescript: "5.9.2",
  vite: "8.3.7",
  vitest: "4.1.12",
});

export const PROBE_DEV_DEPENDENCIES = Object.freeze({
  typescript: "5.9.2",
});

// The audited 7.2.0/14.0.0 tuple was younger than the 1440-minute release-age
// window when frozen. Exempt only these exact versions, retaining the age gate
// for all other dependencies.
export const RELEASE_AGE_EXCLUDES = Object.freeze([
  ...Object.keys(RUNTIME_DEPENDENCIES)
    .filter((name) => name.startsWith("@takazudo/zfb"))
    .map((name) => `${name}@${ZFB_VERSION}`),
  ...Object.values(NATIVE_CARRIERS).map((name) => `${name}@${ZFB_VERSION}`),
  `@takazudo/zfb-slugify@${ZFB_VERSION}`,
  `@takazudo/zudo-doc@${ZUDO_DOC_VERSION}`,
]);

// The target package removed desktop Find entirely. The host owns Find in
// app/src/browser-chrome/find-in-page; this narrow package patch keeps native
// Search shortcut ownership, refresh and the disabled ZDTP runtime boundary.
export const ZUDO_DOC_PATCH = Object.freeze({
  name: "@takazudo/zudo-doc",
  version: ZUDO_DOC_VERSION,
  path: `patches/@takazudo__zudo-doc@${ZUDO_DOC_VERSION}.patch`,
  hash: "c708c28fa1b0822a1a538e887e13d62ec8ba11008bd6df503ceff0f44a0a0098",
});

// zudo-doc dependencies reachable from the package exports the app imports
// (route-context -> extract-headings -> parse5). Every other zudo-doc
// dependency serves its Node CLI or disabled plugins and is not staged.
export const ZUDO_DOC_RUNTIME_DEPENDENCIES = Object.freeze(["parse5"]);

// Packages a migrated scope must not declare directly.
export const FORBIDDEN_DIRECT_DEPENDENCIES = Object.freeze([
  "@takazudo/zfb-adapter-cloudflare",
  "@tailwindcss/vite",
  "@types/react",
  "clsx",
  "gray-matter",
  "mermaid",
  "preact",
  "preact-render-to-string",
  "remark-cjk-friendly",
  "remark-directive",
  "tailwindcss",
]);

export const SCOPES = Object.freeze({
  app: {
    name: "@ccresdoc/app",
    sections: {
      dependencies: RUNTIME_DEPENDENCIES,
      optionalDependencies: CARRIER_DEPENDENCIES,
      devDependencies: APP_DEV_DEPENDENCIES,
    },
    patches: [ZUDO_DOC_PATCH],
  },
  probe: {
    name: "@ccresdoc/node-free-latest-probe",
    sections: {
      dependencies: RUNTIME_DEPENDENCIES,
      optionalDependencies: CARRIER_DEPENDENCIES,
      devDependencies: PROBE_DEV_DEPENDENCIES,
    },
    // The probe stays an unpatched upstream consumer.
    patches: [],
  },
});
