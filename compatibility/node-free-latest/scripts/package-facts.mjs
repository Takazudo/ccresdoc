import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  createReadStream,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { probeRoot } from "./native-binary.mjs";

const packageNames = [
  "@takazudo/zfb",
  "@takazudo/zfb-md-wasm",
  "@takazudo/zfb-runtime",
  "@takazudo/zudo-doc",
];

const carriers = [
  ["darwin-arm64", "@takazudo/zfb-darwin-arm64", "zfb"],
  ["darwin-x64", "@takazudo/zfb-darwin-x64", "zfb"],
  ["linux-arm64-gnu", "@takazudo/zfb-linux-arm64-gnu", "zfb"],
  ["linux-x64-gnu", "@takazudo/zfb-linux-x64-gnu", "zfb"],
  ["win32-x64-msvc", "@takazudo/zfb-win32-x64-msvc", "zfb.exe"],
];

async function sha256(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

export async function collectPackageFacts() {
  const manifest = JSON.parse(readFileSync(join(probeRoot, "package.json"), "utf8"));
  const requested = [
    ...packageNames.map((name) => [name, manifest.dependencies[name]]),
    ...carriers.map(([, name]) => [name, manifest.optionalDependencies[name]]),
  ];
  const packDir = mkdtempSync(join(tmpdir(), "ccresdoc-package-facts-"));

  try {
    const packed = spawnSync("npm", [
      "pack",
      ...requested.map(([name, version]) => `${name}@${version}`),
      "--json",
      "--ignore-scripts",
      "--pack-destination",
      packDir,
    ], {
      cwd: probeRoot,
      encoding: "utf8",
      maxBuffer: 20 * 1024 * 1024,
    });
    if (packed.error) throw packed.error;
    if (packed.status !== 0) {
      throw new Error(`npm pack failed (${packed.status})\n${packed.stdout}${packed.stderr}`);
    }

    const records = JSON.parse(packed.stdout);
    const recordByName = new Map(records.map((record) => [record.name, record]));
    const packages = {};
    for (const [name, version] of requested.slice(0, packageNames.length)) {
      const record = recordByName.get(name);
      if (!record || record.version !== version) throw new Error(`missing npm pack record for ${name}@${version}`);
      packages[name] = { version: record.version, integrity: record.integrity };
    }

    const nativeCarriers = {};
    for (const [platform, name, binaryName] of carriers) {
      const version = manifest.optionalDependencies[name];
      const record = recordByName.get(name);
      if (!record || record.version !== version) throw new Error(`missing npm pack record for ${name}@${version}`);
      const binary = record.files.find((file) => file.path === binaryName);
      if (!binary) throw new Error(`${name}@${version} does not contain ${binaryName}`);

      const extractDir = join(packDir, platform);
      mkdirSync(extractDir);
      const extracted = spawnSync("tar", [
        "-xzf",
        join(packDir, record.filename),
        "-C",
        extractDir,
        `package/${binaryName}`,
      ], { encoding: "utf8" });
      if (extracted.error) throw extracted.error;
      if (extracted.status !== 0) {
        throw new Error(`tar extraction failed for ${name}@${version}\n${extracted.stdout}${extracted.stderr}`);
      }

      const binaryPath = join(extractDir, "package", binaryName);
      const extractedStat = statSync(binaryPath);
      if (extractedStat.size !== binary.size) throw new Error(`${name}@${version} binary size changed during extraction`);
      nativeCarriers[platform] = {
        package: `${name}@${version}`,
        integrity: record.integrity,
        relativePath: `node_modules/${name}/${binaryName}`,
        archiveMode: (binary.mode & 0o777).toString(8).padStart(4, "0"),
        sizeBytes: binary.size,
        sha256: await sha256(binaryPath),
      };
    }

    return { packages, nativeCarriers };
  } finally {
    rmSync(packDir, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  console.log(JSON.stringify(await collectPackageFacts(), null, 2));
}
