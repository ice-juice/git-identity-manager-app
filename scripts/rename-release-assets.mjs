/**
 * 把 Tauri 安装包改成「Git.Keymaster_{版本}_{架构}_{locale}」成对文件名。
 * 用法：node scripts/rename-release-assets.mjs zh-CN|en-US
 */
import fs from "node:fs";
import path from "node:path";

const LOCALES = ["zh-CN", "en-US"];
const BRAND = "Git.Keymaster";

const COMPOUND_SUFFIXES = [
  ".app.tar.gz.sig",
  ".app.tar.gz",
  ".tar.gz.sig",
  ".tar.gz",
  "-setup.exe.sig",
  "-setup.exe",
  ".msi.sig",
  ".msi",
  ".dmg.sig",
  ".dmg",
  ".deb.sig",
  ".deb",
  ".rpm.sig",
  ".rpm",
  ".AppImage.sig",
  ".AppImage",
  ".exe.sig",
  ".exe",
];

export function stampLocaleFilename(name, locale) {
  if (!LOCALES.includes(locale)) {
    throw new Error(`unsupported locale: ${locale}`);
  }
  if (!name || name === "latest.json") {
    return name;
  }

  const suffix = COMPOUND_SUFFIXES.find((s) => name.endsWith(s));
  if (!suffix) {
    return name;
  }

  let stem = name.slice(0, -suffix.length);
  stem = stem.replace(/ /g, ".");
  if (stem.startsWith("-") || stem.startsWith("_") || /^\d/.test(stem)) {
    stem = `${BRAND}_${stem.replace(/^[-_]+/, "")}`;
  }
  stem = stem.replace(/^Git\.?Keymaster\.?/i, `${BRAND}_`);
  stem = stem.replace(/_+/g, "_").replace(/_$/g, "");

  stem = stem.replace(/_(zh-CN|en-US)$/i, "");
  return `${stem}_${locale}${suffix}`;
}

export function rewriteLatestJson(text, mapping) {
  let out = text;
  for (const [from, to] of mapping) {
    if (from === to) continue;
    out = out.split(from).join(to);
  }
  return out;
}

function walkFiles(dir, out = []) {
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

function renameBundles(locale, roots) {
  const mapping = [];
  for (const root of roots) {
    for (const file of walkFiles(root)) {
      const base = path.basename(file);
      if (base === "latest.json") {
        continue;
      }
      const next = stampLocaleFilename(base, locale);
      if (next === base) {
        continue;
      }
      const dest = path.join(path.dirname(file), next);
      if (fs.existsSync(dest) && dest !== file) {
        fs.unlinkSync(dest);
      }
      fs.renameSync(file, dest);
      mapping.push([base, next]);
      console.log(`[rename] ${base} -> ${next}`);
    }
  }
  return mapping;
}

function patchLatestJsonFiles(cwd, mapping) {
  const candidates = [
    path.join(cwd, "latest.json"),
    path.join(cwd, "app", "src-tauri", "target", "release", "latest.json"),
    path.join(cwd, "app", "src-tauri", "target", "universal-apple-darwin", "release", "latest.json"),
  ];
  for (const file of walkFiles(path.join(cwd, "app", "src-tauri", "target"))) {
    if (path.basename(file) === "latest.json") {
      candidates.push(file);
    }
  }
  const seen = new Set();
  for (const file of candidates) {
    if (!fs.existsSync(file) || seen.has(file)) {
      continue;
    }
    seen.add(file);
    const before = fs.readFileSync(file, "utf-8");
    const after = rewriteLatestJson(before, mapping);
    if (after !== before) {
      fs.writeFileSync(file, after, "utf-8");
      console.log(`[rename] patched ${path.relative(cwd, file)}`);
    }
  }
}

function runSelfTest() {
  const cases = [
    ["Git.Keymaster_1.2.0_x64-setup.exe", "en-US", "Git.Keymaster_1.2.0_x64_en-US-setup.exe"],
    ["Git.Keymaster_1.2.0_x64-setup.exe", "zh-CN", "Git.Keymaster_1.2.0_x64_zh-CN-setup.exe"],
    ["Git.Keymaster_1.2.0_x64_en-US.msi", "en-US", "Git.Keymaster_1.2.0_x64_en-US.msi"],
    ["Git.Keymaster_1.2.0_x64_en-US.msi", "zh-CN", "Git.Keymaster_1.2.0_x64_zh-CN.msi"],
    ["Git Keymaster_1.2.0_x64-setup.exe.sig", "en-US", "Git.Keymaster_1.2.0_x64_en-US-setup.exe.sig"],
    ["_1.2.0_amd64.AppImage", "zh-CN", "Git.Keymaster_1.2.0_amd64_zh-CN.AppImage"],
    ["-1.2.0-1.x86_64.rpm", "zh-CN", "Git.Keymaster_1.2.0-1.x86_64_zh-CN.rpm"],
    ["Git.Keymaster_1.2.0_amd64.deb", "en-US", "Git.Keymaster_1.2.0_amd64_en-US.deb"],
    ["Git.Keymaster_1.2.0_universal.dmg", "zh-CN", "Git.Keymaster_1.2.0_universal_zh-CN.dmg"],
    ["Git.Keymaster_universal.app.tar.gz", "en-US", "Git.Keymaster_universal_en-US.app.tar.gz"],
    ["Git.Keymaster_1.2.0_universal.app.tar.gz.sig", "zh-CN", "Git.Keymaster_1.2.0_universal_zh-CN.app.tar.gz.sig"],
    ["latest.json", "zh-CN", "latest.json"],
  ];
  for (const [input, locale, expected] of cases) {
    const got = stampLocaleFilename(input, locale);
    if (got !== expected) {
      throw new Error(`stamp ${input} / ${locale} => ${got}, expected ${expected}`);
    }
  }
  const rewritten = rewriteLatestJson(
    "https://example/Git.Keymaster_1.2.0_x64-setup.exe",
    [["Git.Keymaster_1.2.0_x64-setup.exe", "Git.Keymaster_1.2.0_x64_zh-CN-setup.exe"]],
  );
  if (!rewritten.endsWith("Git.Keymaster_1.2.0_x64_zh-CN-setup.exe")) {
    throw new Error("latest.json rewrite failed");
  }
  console.log("[rename] self-test ok");
}

const arg = process.argv[2];
if (arg === "--test") {
  runSelfTest();
} else if (LOCALES.includes(arg)) {
  const cwd = process.cwd();
  const roots = [
    path.join(cwd, "app", "src-tauri", "target", "release", "bundle"),
    path.join(cwd, "app", "src-tauri", "target", "universal-apple-darwin", "release", "bundle"),
  ];
  const mapping = renameBundles(arg, roots);
  fs.writeFileSync(path.join(cwd, "renamed-release-assets.json"), JSON.stringify(mapping, null, 2), "utf-8");
  patchLatestJsonFiles(cwd, mapping);
} else {
  console.error("usage: node scripts/rename-release-assets.mjs zh-CN|en-US|--test");
  process.exit(1);
}
