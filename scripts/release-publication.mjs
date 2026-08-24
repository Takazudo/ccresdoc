#!/usr/bin/env node

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { artifactNames, normalizeTag } from "./release-contract.mjs";

const SHA_PATTERN = /^[0-9a-f]{40}$/;
const TAG_PATTERN = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

export class ReleasePublicationError extends Error {
  constructor(message) {
    super(message);
    this.name = "ReleasePublicationError";
  }
}

function reject(message) {
  throw new ReleasePublicationError(message);
}

function object(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    reject(`${label} must be an object`);
  }
  return value;
}

function string(value, label) {
  if (typeof value !== "string" || value.length === 0) reject(`${label} must be a non-empty string`);
  return value;
}

function sha(value, label) {
  if (typeof value !== "string" || !SHA_PATTERN.test(value)) {
    reject(`${label} must be a full lowercase 40-hex SHA`);
  }
  return value;
}

function databaseId(value, label) {
  if (!Number.isSafeInteger(value) || value <= 0) reject(`${label} must be a positive integer`);
  return value;
}

function boolean(value, label) {
  if (typeof value !== "boolean") reject(`${label} must be a boolean`);
  return value;
}

function validateSource(snapshot, tag) {
  const source = object(snapshot.source, "source");
  const version = string(source.version, "source.version");
  const normalizedSourceTag = normalizeTag(version);
  if (source.tag !== normalizedSourceTag) reject(`source.tag must equal ${normalizedSourceTag}`);
  if (tag !== normalizedSourceTag) reject(`requested tag ${tag} does not match source tag ${normalizedSourceTag}`);
  const expected = artifactNames(version);
  if (source.artifactName !== expected.artifactName) {
    reject(`source.artifactName must equal ${expected.artifactName}`);
  }
  if (source.checksumName !== expected.checksumName) {
    reject(`source.checksumName must equal ${expected.checksumName}`);
  }
  return { version, ...expected };
}

function validateTag(tagState, targetSha) {
  const state = object(tagState, "remoteTag");
  if (!new Set(["absent", "lightweight", "annotated"]).has(state.kind)) {
    reject("remoteTag.kind must be absent, lightweight, or annotated");
  }
  if (state.kind === "absent") {
    if (state.objectSha !== null || state.peeledSha !== null) {
      reject("an absent remote tag cannot contain object SHAs");
    }
    return "absent";
  }
  const objectSha = sha(state.objectSha, "remoteTag.objectSha");
  const peeledSha = sha(state.peeledSha, "remoteTag.peeledSha");
  if (state.kind === "lightweight" && objectSha !== peeledSha) {
    reject("lightweight tag object and peeled SHAs must match");
  }
  if (state.kind === "annotated" && objectSha === peeledSha) {
    reject("annotated tag object and peeled SHAs must differ");
  }
  if (peeledSha !== targetSha) reject(`remote tag peels to ${peeledSha}, not target ${targetSha}`);
  return state.kind;
}

function validateCi(ci, targetSha) {
  const evidence = object(ci, "ci");
  if (evidence.workflowPath !== ".github/workflows/ci.yml") reject("CI evidence has the wrong workflow path");
  if (evidence.workflowName !== "CI") reject("CI evidence has the wrong workflow name");
  if (evidence.event !== "push") reject("CI evidence must have event=push");
  if (evidence.branch !== "main") reject("CI evidence must have branch=main");
  if (evidence.headSha !== targetSha) reject("CI evidence head SHA does not match the target");
  if (evidence.status !== "completed") reject("CI evidence must be completed");
  if (evidence.conclusion !== "success") reject("CI evidence must have a successful conclusion");
  databaseId(evidence.databaseId, "ci.databaseId");
}

function validateAssets(release, expectedNames, expected) {
  if (!Array.isArray(release.assets)) reject("release.assets must be an array");
  const assets = release.assets.map((entry, index) => {
    const asset = object(entry, `release.assets[${index}]`);
    return {
      id: databaseId(asset.id, `release.assets[${index}].id`),
      name: string(asset.name, `release.assets[${index}].name`),
    };
  });
  const names = assets.map(({ name }) => name);
  if (new Set(names).size !== names.length) reject("release asset names must be unique");
  const wanted = [expectedNames.artifactName, expectedNames.checksumName].sort();
  if (names.length !== wanted.length || names.slice().sort().some((name, index) => name !== wanted[index])) {
    reject(`release assets must be exactly: ${wanted.join(", ")}`);
  }
  const assetIds = Object.fromEntries(assets.map(({ name, id }) => [name, id]));
  if (expected?.assetIds) {
    const prior = object(expected.assetIds, "expected.assetIds");
    for (const name of wanted) {
      if (prior[name] !== assetIds[name]) reject(`release asset ${name} no longer has the validated database ID`);
    }
  }
  return assetIds;
}

export function validateChecksumText(textValue, artifactName) {
  const text = string(textValue, "checksum.text");
  const match = /^([0-9a-f]{64})  ([^\r\n]+)\n$/.exec(text);
  if (!match) reject("checksum text must be one lowercase GNU sha256sum line ending in a newline");
  if (match[2] !== artifactName) reject(`checksum line must name ${artifactName}`);
  return text;
}

function validateChecksum(checksum, artifactName) {
  const evidence = object(checksum, "checksum");
  validateChecksumText(evidence.text, artifactName);
  if (!boolean(evidence.verified, "checksum.verified")) reject("sha256sum verification evidence is false");
}

export function validatePublicationSnapshot(value, options = {}) {
  const snapshot = object(value, "snapshot");
  const tag = string(snapshot.tag, "tag");
  if (!TAG_PATTERN.test(tag)) reject("tag must be strict v<stable-semver>");
  const targetSha = sha(snapshot.targetSha, "targetSha");
  const source = validateSource(snapshot, tag);
  if (sha(snapshot.mainSha, "mainSha") !== targetSha) reject("current main does not equal the target SHA");
  const tagKind = validateTag(snapshot.remoteTag, targetSha);
  validateCi(snapshot.ci, targetSha);

  const release = object(snapshot.release, "release");
  const releaseDatabaseId = databaseId(release.databaseId, "release.databaseId");
  if (options.expected?.releaseDatabaseId !== undefined
    && options.expected.releaseDatabaseId !== releaseDatabaseId) {
    reject("release database ID no longer matches the validated draft");
  }
  if (release.tagName !== tag) reject("release tag does not match the requested tag");
  if (release.targetCommitish !== targetSha) reject("release target does not exactly match the target SHA");
  if (boolean(release.prerelease, "release.prerelease")) reject("stable release cannot be a prerelease");
  const draft = boolean(release.draft, "release.draft");
  const latest = boolean(release.latest, "release.latest");
  const assetIds = validateAssets(release, source, options.expected);
  validateChecksum(snapshot.checksum, source.artifactName);

  const phase = options.phase ?? "initial";
  if (!new Set(["initial", "prepublish", "published"]).has(phase)) reject(`unknown validation phase: ${phase}`);
  if (phase === "prepublish" && !draft) reject("validated draft became published before publication mutation");
  if (phase === "published" && draft) reject("release is still a draft after publication");
  if (!draft && tagKind === "absent") reject("a published release requires an existing exact tag");
  if (!draft && !latest) reject("a published stable release must be Latest");

  return {
    disposition: draft ? "draft" : "already_complete",
    releaseDatabaseId,
    artifactName: source.artifactName,
    checksumName: source.checksumName,
    assetIds,
  };
}

export function runCli(argumentsList = process.argv.slice(2)) {
  try {
    const [command, snapshotPath, expectedPath] = argumentsList;
    if (command === "check-checksum" && snapshotPath && expectedPath && argumentsList.length === 3) {
      validateChecksumText(readFileSync(resolve(snapshotPath), "utf8"), expectedPath);
      return 0;
    }
    if (command !== "validate" || !snapshotPath || argumentsList.length > 3) {
      reject("usage: release-publication.mjs validate <snapshot.json> [expected.json] | check-checksum <file> <artifact-name>");
    }
    const snapshot = JSON.parse(readFileSync(resolve(snapshotPath), "utf8"));
    const expectedEnvelope = expectedPath
      ? JSON.parse(readFileSync(resolve(expectedPath), "utf8"))
      : {};
    const result = validatePublicationSnapshot(snapshot, {
      phase: expectedEnvelope.phase,
      expected: expectedEnvelope.expected,
    });
    process.stdout.write(`${JSON.stringify(result)}\n`);
    return 0;
  } catch (error) {
    process.stderr.write(`release-publication: ${error instanceof Error ? error.message : String(error)}\n`);
    return 1;
  }
}

const isMainModule = process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url));
if (isMainModule) process.exitCode = runCli();
