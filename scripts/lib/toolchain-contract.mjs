// Audited published-toolchain tuple for the app and the compatibility probe.
// These values are written by hand after registry review; validators compare
// manifests, lockfiles and installed trees against them, never against
// values read back from the manifests themselves.

export const ZFB_VERSION = "6.0.1";
export const ZUDO_DOC_VERSION = "10.0.2";
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
  vitest: "4.1.12",
});

export const PROBE_DEV_DEPENDENCIES = Object.freeze({
  typescript: "5.9.2",
});

// zudo-doc 10.0.2 was younger than the 1440-minute release-age window when
// this tuple was frozen; the ZFB family was not.
export const RELEASE_AGE_EXCLUDES = Object.freeze([`@takazudo/zudo-doc@${ZUDO_DOC_VERSION}`]);

export const ZUDO_DOC_PATCH = Object.freeze({
  name: "@takazudo/zudo-doc",
  version: ZUDO_DOC_VERSION,
  path: `patches/@takazudo__zudo-doc@${ZUDO_DOC_VERSION}.patch`,
  hash: "d6f52919667c2478934b2e35714e18220341dfd939c9b4f8f9a6317e0de2e5c9",
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
