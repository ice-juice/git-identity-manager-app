/**
 * 拦住会反复出现的发版低级错误：中文包显示英文名、CFBundleName 写成 ASCII 标识符等。
 *
 *   node scripts/check-release-invariants.mjs
 *   node scripts/check-release-invariants.mjs --prepared zh|en
 *   node scripts/check-release-invariants.mjs --test
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  displayNameFor,
  EN_DISPLAY_NAME,
  INSTALLER_STEM,
  MAIN_BINARY,
  WIN_INSTALL_DIR,
  ZH_DISPLAY_NAME,
} from "./brand.mjs";

const __filename = fileURLToPath(import.meta.url);
const rootDir = path.resolve(path.dirname(__filename), "..");

function read(rel) {
  return fs.readFileSync(path.join(rootDir, rel), "utf-8");
}

function plistString(xml, key) {
  const m = xml.match(new RegExp(`<key>${key}</key>\\s*<string>([^<]*)</string>`));
  return m ? m[1] : null;
}

function fail(message) {
  throw new Error(message);
}

function expectEq(actual, expected, label) {
  if (actual !== expected) {
    fail(`${label} 应为「${expected}」，实际是「${actual}」`);
  }
}

export function assertPreparedFiles(lang) {
  const expected = displayNameFor(lang);
  const tauriConf = JSON.parse(read("app/src-tauri/tauri.conf.json"));
  expectEq(tauriConf.productName, expected, "tauri.conf.json productName");
  expectEq(tauriConf.app?.windows?.[0]?.title, expected, "tauri.conf.json window title");
  expectEq(tauriConf.mainBinaryName, MAIN_BINARY, "tauri.conf.json mainBinaryName");

  const plist = read("app/src-tauri/Info.plist");
  expectEq(plistString(plist, "CFBundleDisplayName"), expected, "Info.plist CFBundleDisplayName");
  expectEq(plistString(plist, "CFBundleName"), expected, "Info.plist CFBundleName");
}

function checkCommittedDefaults() {
  assertPreparedFiles("zh");

  const nsi = read("app/src-tauri/windows/installer.nsi");
  if (!nsi.includes(`!define INSTALLDIRNAME "${WIN_INSTALL_DIR}"`)) {
    fail(`installer.nsi 的默认安装目录必须是 ASCII「${WIN_INSTALL_DIR}」，不要用显示名当路径`);
  }
}

function checkPrepareLangSource() {
  const src = read("scripts/prepare-lang.mjs");
  if (!src.includes('upsertPlistString(plist, "CFBundleDisplayName", displayName)')) {
    fail("prepare-lang.mjs 必须用 displayName 写 CFBundleDisplayName");
  }
  if (!src.includes('upsertPlistString(plist, "CFBundleName", displayName)')) {
    fail("prepare-lang.mjs 必须用 displayName 写 CFBundleName，禁止写死 GitKeymaster / Git Keymaster");
  }
  if (/upsertPlistString\(\s*plist\s*,\s*["']CFBundleName["']\s*,\s*["']/.test(src)) {
    fail("prepare-lang.mjs 把 CFBundleName 写成了字符串字面量；必须跟 displayName 走");
  }
  if (!src.includes("from \"./brand.mjs\"") && !src.includes("from './brand.mjs'")) {
    fail("prepare-lang.mjs 必须从 scripts/brand.mjs 取显示名，不要再复制一份");
  }
}

function checkUiSource() {
  const config = read("app/src/lib/config.ts");
  if (!config.includes("export function pickBrandName")) {
    fail("app/src/lib/config.ts 必须导出 pickBrandName，中文包左上角不得回落到英文运行时名");
  }
  if (!config.includes('if (lang === "zh") return "御钥师"') && !config.includes("if (lang === 'zh') return '御钥师'")) {
    fail("pickBrandName 在 lang===zh 时必须返回「御钥师」");
  }

  const titleBar = read("app/src/ui/TitleBar.tsx");
  if (!titleBar.includes("pickBrandName")) {
    fail("TitleBar 必须用 pickBrandName，不能直接渲染 getName() / 英文 productName");
  }
}

function checkRenameStem() {
  const src = read("scripts/rename-release-assets.mjs");
  if (!src.includes("from \"./brand.mjs\"") && !src.includes("from './brand.mjs'")) {
    fail("rename-release-assets.mjs 必须从 scripts/brand.mjs 取安装包文件名主干");
  }
  if (!src.includes("INSTALLER_STEM")) {
    fail(`安装包文件名主干必须使用 brand.mjs 的 INSTALLER_STEM（${INSTALLER_STEM}）`);
  }
}

export function checkSourceInvariants() {
  checkCommittedDefaults();
  checkPrepareLangSource();
  checkUiSource();
  checkRenameStem();
}

function runSelfTest() {
  if (displayNameFor("zh") !== ZH_DISPLAY_NAME || displayNameFor("en") !== EN_DISPLAY_NAME) {
    fail("displayNameFor 语言映射错了");
  }
  if (displayNameFor("en-US") !== EN_DISPLAY_NAME) {
    fail("en-US 应映射到英文显示名");
  }
  const xml = `<key>CFBundleName</key>\n\t<string>${ZH_DISPLAY_NAME}</string>`;
  if (plistString(xml, "CFBundleName") !== ZH_DISPLAY_NAME) {
    fail("plist 解析自测失败");
  }
  const badPrepare = `upsertPlistString(plist, "CFBundleName", "GitKeymaster");`;
  if (!/upsertPlistString\(\s*plist\s*,\s*["']CFBundleName["']\s*,\s*["']/.test(badPrepare)) {
    fail("没能识别把 CFBundleName 写死成英文的旧写法");
  }
  checkSourceInvariants();
  console.log("[invariants] self-test ok");
}

const invoked = process.argv[1] && path.basename(process.argv[1]) === "check-release-invariants.mjs";
if (invoked) {
  try {
    const arg = process.argv[2];
    if (arg === "--test") {
      runSelfTest();
    } else if (arg === "--prepared") {
      const lang = process.argv[3];
      if (!lang) {
        fail("usage: node scripts/check-release-invariants.mjs --prepared zh|en");
      }
      assertPreparedFiles(lang);
      console.log(`[invariants] prepared ${lang} displayName=${displayNameFor(lang)}`);
    } else if (!arg) {
      checkSourceInvariants();
      console.log("[invariants] source ok");
    } else {
      console.error("usage: node scripts/check-release-invariants.mjs [--prepared zh|en|--test]");
      process.exit(1);
    }
  } catch (err) {
    console.error(`[invariants] ${err.message}`);
    process.exit(1);
  }
}
