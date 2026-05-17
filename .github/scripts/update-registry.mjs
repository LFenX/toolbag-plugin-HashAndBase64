#!/usr/bin/env node
/**
 * Merge a new release into a Toolbag-Registry checkout.
 *
 * Usage:
 *   node update-registry.mjs <registry-root> <plugin-root>
 *
 * Required env:
 *   PLUGIN_REPO          owner/repo of this plugin (e.g. LFenX/toolbag-plugin-...)
 *   PLUGIN_TAG           git tag for this release (e.g. v0.1.0)
 *   PLUGIN_ID            plugin id (matches tool.json.id)
 *   PLUGIN_VERSION       semver (matches tool.json.version)
 *   PLUGIN_ARCHIVE_NAME  file name of the .tbpkg on the release (e.g.
 *                        toolbag-plugin-com-lfen-toolbag-hash-and-base64-0.1.0.tbpkg)
 *   PLUGIN_SHA256        hex sha256 of the .tbpkg
 *
 * Behaviour:
 *   - Reads <plugin-root>/tool.json for top-level fields (name, description,
 *     category, tags, minAppVersion, riskLevel, icon).
 *   - Reads <plugin-root>/changelog.md and grabs the first H2 section as
 *     changelog text for this release entry.
 *   - Loads <registry-root>/plugins/<id>.json (or starts a fresh document
 *     when missing) and prepends the new release to `releases[]`. If the
 *     same version is already present, it's replaced (idempotent re-runs).
 *   - Writes the JSON back with stable formatting (2-space indent, trailing
 *     newline) so diffs in the PR stay readable.
 */

import fs from "node:fs";
import path from "node:path";

function die(message) {
  console.error(`update-registry: ${message}`);
  process.exit(1);
}

function requireEnv(name) {
  const value = process.env[name];
  if (!value || value.trim() === "") die(`missing env ${name}`);
  return value;
}

function readJson(file) {
  return JSON.parse(fs.readFileSync(file, "utf8"));
}

function writeJson(file, doc) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, JSON.stringify(doc, null, 2) + "\n", "utf8");
}

function extractChangelogSection(text, version) {
  if (!text) return null;
  const lines = text.split(/\r?\n/);
  // Find an H2 line that mentions the target version (e.g. "## 0.1.0 — 2026-05-17").
  // If we can't find one tied to this version, fall back to the first H2 section.
  let startIdx = lines.findIndex(
    (line) => /^##\s+/.test(line) && line.includes(version),
  );
  if (startIdx === -1) {
    startIdx = lines.findIndex((line) => /^##\s+/.test(line));
  }
  if (startIdx === -1) return null;
  let endIdx = lines.length;
  for (let i = startIdx + 1; i < lines.length; i += 1) {
    if (/^##\s+/.test(lines[i])) {
      endIdx = i;
      break;
    }
  }
  return lines
    .slice(startIdx + 1, endIdx)
    .join("\n")
    .trim() || null;
}

function main() {
  const [registryRoot, pluginRoot] = process.argv.slice(2);
  if (!registryRoot || !pluginRoot) {
    die("usage: update-registry.mjs <registry-root> <plugin-root>");
  }

  const repo = requireEnv("PLUGIN_REPO");
  const tag = requireEnv("PLUGIN_TAG");
  const id = requireEnv("PLUGIN_ID");
  const version = requireEnv("PLUGIN_VERSION");
  const archiveName = requireEnv("PLUGIN_ARCHIVE_NAME");
  const sha256 = requireEnv("PLUGIN_SHA256");

  const manifestPath = path.join(pluginRoot, "tool.json");
  if (!fs.existsSync(manifestPath)) die(`tool.json not found at ${manifestPath}`);
  const manifest = readJson(manifestPath);

  if (manifest.id !== id) {
    die(`tool.json id (${manifest.id}) != PLUGIN_ID (${id})`);
  }
  if (manifest.version !== version) {
    die(`tool.json version (${manifest.version}) != PLUGIN_VERSION (${version})`);
  }

  const baseReleaseUrl = `https://github.com/${repo}/releases/download/${tag}`;
  const downloadUrl = `${baseReleaseUrl}/${archiveName}`;
  const signatureUrl = `${downloadUrl}.sig`;

  const changelogPath = path.join(pluginRoot, "changelog.md");
  const changelogText = fs.existsSync(changelogPath)
    ? fs.readFileSync(changelogPath, "utf8")
    : null;
  const changelog = extractChangelogSection(changelogText, version);

  const iconUrl =
    manifest.icon && typeof manifest.icon === "string" && manifest.icon.trim()
      ? `https://raw.githubusercontent.com/${repo}/${tag}/${manifest.icon}`
      : null;

  const entry = {
    version,
    minAppVersion: manifest.minAppVersion ?? null,
    publishedAt: new Date().toISOString(),
    downloadUrl,
    signatureUrl,
    sha256: sha256.toLowerCase(),
  };
  if (changelog) entry.changelog = changelog;

  const docPath = path.join(registryRoot, "plugins", `${id}.json`);
  let doc;
  if (fs.existsSync(docPath)) {
    doc = readJson(docPath);
    if (doc.id && doc.id !== id) {
      die(`existing ${docPath} has id ${doc.id}, refusing to overwrite`);
    }
  } else {
    doc = {
      id,
      repo,
      releases: [],
    };
  }

  // Always refresh top-level fields from the latest manifest so the registry
  // reflects renames / category changes / icon updates without manual edits.
  doc.id = id;
  doc.repo = repo;
  doc.name = manifest.name;
  doc.description = manifest.description;
  if (manifest.detailDescription) doc.detailDescription = manifest.detailDescription;
  doc.category = manifest.category;
  doc.tags = Array.isArray(manifest.tags) ? manifest.tags : [];
  doc.riskLevel = manifest.riskLevel ?? "safe";
  if (iconUrl) doc.iconUrl = iconUrl;
  if (manifest.author) doc.author = manifest.author;
  if (manifest.homepage) doc.homepage = manifest.homepage;
  if (manifest.license) doc.license = manifest.license;

  const releases = Array.isArray(doc.releases) ? doc.releases : [];
  // Replace any existing record of this version (idempotent re-runs).
  const filtered = releases.filter((release) => release.version !== version);
  filtered.unshift(entry);
  doc.releases = filtered;

  writeJson(docPath, doc);
  console.log(`wrote ${docPath}`);
  console.log(JSON.stringify(entry, null, 2));
}

main();
