import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";

const version = normalizeVersion(process.argv[2]);
const assetsDir = process.argv[3];
if (!assetsDir) {
  throw new Error("Usage: node scripts/update-release-readme.mjs <version> <assets-directory>");
}

const files = [
  `codex-qianzong_${version}_x64.msi`,
  `codex-qianzong_${version}_aarch64.dmg`,
  `codex-qianzong_${version}_x86_64.dmg`,
];
const checksums = files.map((file) => ({ file, sha256: sha256(join(assetsDir, file)) }));

const startMarker = "<!-- release-verification:start -->";
const endMarker = "<!-- release-verification:end -->";
const readme = readFileSync("README.md", "utf8");
const start = readme.indexOf(startMarker);
const end = readme.indexOf(endMarker);
if (start < 0 || end < start) {
  throw new Error("README release verification markers are missing or out of order");
}

const block = renderReadmeBlock(version, checksums, startMarker, endMarker);
const updated = `${readme.slice(0, start)}${block}${readme.slice(end + endMarker.length)}`;
writeFileSync("README.md", updated);
writeFileSync(
  join(assetsDir, "SHA256SUMS.txt"),
  `${checksums.map(({ file, sha256: hash }) => `${hash}  ${file}`).join("\n")}\n`,
);
writeFileSync("release-notes.md", renderReleaseNotes(version, checksums));

process.stdout.write(`Updated README and checksums for v${version}\n`);

function normalizeVersion(input) {
  const normalized = String(input ?? "")
    .trim()
    .replace(/^v/, "");
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(normalized)) {
    throw new Error("Version must look like 1.5.3 or 1.5.3-rc.1");
  }
  return normalized;
}

function sha256(path) {
  if (!existsSync(path)) {
    throw new Error(`Missing release asset: ${path}`);
  }
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function renderReadmeBlock(releaseVersion, entries, start, end) {
  const rows = entries.map(({ file, sha256: hash }) => `| \`${file}\` | \`${hash}\` |`).join("\n");
  return `${start}
## 最新发布校验 / Latest Release Verification

当前版本 / Current version: \`v${releaseVersion}\`

| 文件 / File | SHA-256 |
| --- | --- |
${rows}

附注：

- Apple Silicon 请下载 \`codex-qianzong_${releaseVersion}_aarch64.dmg\`
- Mac Intel 请下载 \`codex-qianzong_${releaseVersion}_x86_64.dmg\`

Notes:

- Apple Silicon: download \`codex-qianzong_${releaseVersion}_aarch64.dmg\`
- Mac Intel: download \`codex-qianzong_${releaseVersion}_x86_64.dmg\`
${end}`;
}

function renderReleaseNotes(releaseVersion, entries) {
  const checksums = entries
    .map(({ file, sha256: hash }) => `- \`${file}\`: \`${hash}\``)
    .join("\n");
  return `## v${releaseVersion}

### Downloads

- Windows x64: \`codex-qianzong_${releaseVersion}_x64.msi\`
- Apple Silicon: \`codex-qianzong_${releaseVersion}_aarch64.dmg\`
- Mac Intel: \`codex-qianzong_${releaseVersion}_x86_64.dmg\`

### SHA-256

${checksums}

The macOS builds are ad-hoc signed and are not Apple-notarized.
`;
}
