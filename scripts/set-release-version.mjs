import { readFileSync, writeFileSync } from "node:fs";
import process from "node:process";

const version = normalizeVersion(process.argv[2]);

updateJson("package.json", (value) => {
  value.version = version;
});
updateJson("package-lock.json", (value) => {
  value.version = version;
  value.packages[""].version = version;
});
updateJson("src-tauri/tauri.conf.json", (value) => {
  value.version = version;
});
updateCargoManifest("src-tauri/Cargo.toml", version);
updateCargoLock("src-tauri/Cargo.lock", version);

process.stdout.write(`Updated project version to ${version}\n`);

function normalizeVersion(input) {
  const normalized = String(input ?? "")
    .trim()
    .replace(/^v/, "");
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(normalized)) {
    throw new Error("Version must look like 1.5.3 or 1.5.3-rc.1");
  }
  return normalized;
}

function updateJson(path, update) {
  const value = JSON.parse(readFileSync(path, "utf8"));
  update(value);
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

function updateCargoManifest(path, nextVersion) {
  const source = readFileSync(path, "utf8");
  const packageStart = source.indexOf("[package]");
  const packageEnd = source.indexOf("\n[", packageStart + 1);
  if (packageStart < 0 || packageEnd < 0) {
    throw new Error(`Could not find [package] section in ${path}`);
  }

  const section = source.slice(packageStart, packageEnd);
  const versionPattern = /^version\s*=\s*"[^"]+"/m;
  if (!versionPattern.test(section)) {
    throw new Error(`Could not update package version in ${path}`);
  }
  const updated = section.replace(versionPattern, `version = "${nextVersion}"`);
  writeFileSync(path, source.slice(0, packageStart) + updated + source.slice(packageEnd));
}

function updateCargoLock(path, nextVersion) {
  const source = readFileSync(path, "utf8");
  const pattern = /(\[\[package\]\]\r?\nname = "codex-qianzong"\r?\nversion = ")[^"]+("\r?\n)/;
  if (!pattern.test(source)) {
    throw new Error(`Could not find codex-qianzong package in ${path}`);
  }
  writeFileSync(path, source.replace(pattern, `$1${nextVersion}$2`));
}
