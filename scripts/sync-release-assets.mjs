/**
 * 把带语言后缀的安装包同步到 GitHub Release，并删掉未加后缀的旧文件名。
 * 用法：
 *   node scripts/sync-release-assets.mjs vX.Y.Z
 *   node scripts/sync-release-assets.mjs --require-bundles
 *   node scripts/sync-release-assets.mjs --test
 */
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const REPO = "ice-juice/git-keymaster-app";
const BUNDLE_ROOTS = [
  "app/src-tauri/target/release/bundle",
  "app/src-tauri/target/universal-apple-darwin/release/bundle",
];
const INSTALLER_SUFFIXES = [
  ".app.tar.gz",
  ".dmg",
  "-setup.exe",
  ".msi",
  ".deb",
  ".rpm",
  ".AppImage",
];

export function walkFiles(dir, out = []) {
  if (!fs.existsSync(dir)) {
    return out;
  }
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, ent.name);
    if (ent.isDirectory()) {
      walkFiles(p, out);
    } else {
      out.push(p);
    }
  }
  return out;
}

export function collectUniqueUploads(mapping, cwd = process.cwd()) {
  const names = new Set(mapping.map(([, to]) => to));
  names.add("latest.json");
  const byBasename = new Map();
  for (const root of BUNDLE_ROOTS) {
    for (const file of walkFiles(path.join(cwd, root))) {
      const base = path.basename(file);
      if (names.has(base)) {
        byBasename.set(base, file);
      }
    }
  }
  if (!byBasename.has("latest.json")) {
    for (const file of walkFiles(path.join(cwd, "app/src-tauri/target"))) {
      if (path.basename(file) === "latest.json") {
        byBasename.set("latest.json", file);
        break;
      }
    }
  }
  return [...byBasename.values()];
}

export function listInstallerBundles(cwd = process.cwd()) {
  return BUNDLE_ROOTS.flatMap((root) => walkFiles(path.join(cwd, root))).filter((file) =>
    INSTALLER_SUFFIXES.some((suffix) => file.endsWith(suffix) && !file.endsWith(`${suffix}.sig`)),
  );
}

function runGh(args, { ignoreFail = false } = {}) {
  const result = spawnSync("gh", args, { stdio: "inherit" });
  if (result.error) {
    if (ignoreFail) {
      return result.status ?? 1;
    }
    throw result.error;
  }
  if ((result.status ?? 1) !== 0 && !ignoreFail) {
    process.exit(result.status ?? 1);
  }
  return result.status ?? 1;
}

function requireBundles() {
  const files = listInstallerBundles();
  if (files.length === 0) {
    console.error("tauri-action produced no installers");
    process.exit(1);
  }
  for (const file of files) {
    console.log(`[bundle] ${path.relative(process.cwd(), file)}`);
  }
}

function syncTag(tag) {
  if (!tag || tag === "v__VERSION__") {
    console.log("skip release asset sync (no real tag)");
    return;
  }

  const mappingPath = path.join(process.cwd(), "renamed-release-assets.json");
  if (!fs.existsSync(mappingPath)) {
    console.log("no renamed-release-assets.json, nothing to sync");
    return;
  }

  const mapping = JSON.parse(fs.readFileSync(mappingPath, "utf8"));
  if (!Array.isArray(mapping) || mapping.length === 0) {
    console.log("rename mapping empty");
    return;
  }

  const uploads = collectUniqueUploads(mapping);
  if (uploads.length > 0) {
    for (const file of uploads) {
      console.log(`[upload] ${path.basename(file)}`);
    }
    runGh(["release", "upload", tag, ...uploads, "--repo", REPO, "--clobber"]);
  }

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "gam-latest-"));
  try {
    const downloaded = runGh(
      ["release", "download", tag, "--repo", REPO, "--pattern", "latest.json", "--dir", tmp, "--clobber"],
      { ignoreFail: true },
    );
    const latestPath = path.join(tmp, "latest.json");
    if (downloaded === 0 && fs.existsSync(latestPath)) {
      let text = fs.readFileSync(latestPath, "utf8");
      for (const [from, to] of mapping) {
        if (from !== to) {
          text = text.split(from).join(to);
        }
      }
      fs.writeFileSync(latestPath, text);
      runGh(["release", "upload", tag, latestPath, "--repo", REPO, "--clobber"]);
    }
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }

  for (const [oldName, newName] of mapping) {
    if (oldName && oldName !== newName) {
      runGh(["release", "delete-asset", tag, oldName, "--repo", REPO, "--yes"], { ignoreFail: true });
    }
  }
}

function runSelfTest() {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "sync-assets-test-"));
  try {
    const bundle = path.join(tmp, "app/src-tauri/target/release/bundle/nsis");
    fs.mkdirSync(bundle, { recursive: true });
    const locale = "Git.Keymaster_1.3.0_x64_en-US-setup.exe";
    const extra = "Git.Keymaster_1.3.0_x64-setup.exe";
    fs.writeFileSync(path.join(bundle, locale), "a");
    fs.writeFileSync(path.join(bundle, extra), "b");
    const nested = path.join(tmp, "app/src-tauri/target/release/bundle/copy");
    fs.mkdirSync(nested, { recursive: true });
    fs.writeFileSync(path.join(nested, locale), "dup");
    const uploads = collectUniqueUploads(
      [["Git.Keymaster_1.3.0_x64-setup.exe", locale]],
      tmp,
    );
    if (uploads.length !== 1 || path.basename(uploads[0]) !== locale) {
      throw new Error(`expected one unique locale file, got ${JSON.stringify(uploads)}`);
    }
    if (listInstallerBundles(tmp).length < 2) {
      throw new Error("expected installer files under bundle/");
    }
    console.log("[sync] self-test ok");
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

const invoked = process.argv[1] && path.basename(process.argv[1]) === "sync-release-assets.mjs";
if (invoked) {
  const arg = process.argv[2];
  if (arg === "--test") {
    runSelfTest();
  } else if (arg === "--require-bundles") {
    requireBundles();
  } else {
    syncTag(arg);
  }
}
