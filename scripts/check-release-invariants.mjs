/**
 * 拦住会反复出现的发版低级错误：中文包显示英文名、标题栏读 getName()、
 * prepare-lang 把语言写进会污染 `tauri dev` 的 app/.env 等。
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
  if (!src.includes("from \"./brand.mjs\"") && !src.includes("from './brand.mjs'")) {
    fail("prepare-lang.mjs 必须从 scripts/brand.mjs 取显示名，不要再复制一份");
  }
  if (!src.includes('upsertPlistString(plist, "CFBundleDisplayName", displayName)')) {
    fail("prepare-lang.mjs 必须用 displayName 写 CFBundleDisplayName");
  }
  if (!src.includes('upsertPlistString(plist, "CFBundleName", displayName)')) {
    fail("prepare-lang.mjs 必须用 displayName 写 CFBundleName，禁止写死 GitKeymaster / Git Keymaster");
  }
  if (/upsertPlistString\(\s*plist\s*,\s*["']CFBundleName["']\s*,\s*["']/.test(src)) {
    fail("prepare-lang.mjs 把 CFBundleName 写成了字符串字面量；必须跟 displayName 走");
  }
  if (!src.includes(".env.production")) {
    fail("prepare-lang.mjs 只能写 app/.env.production，禁止写 app/.env（会污染 tauri dev）");
  }
  if (/writeFileSync\(\s*envPath\s*,/.test(src) && !src.includes("envProductionPath")) {
    fail("prepare-lang.mjs 看起来仍在写 app/.env；必须改为 .env.production 并删除残留 .env");
  }
  if (!src.includes("unlinkSync") && !src.includes("rmSync")) {
    fail("prepare-lang.mjs 必须删掉残留的 app/.env，避免 VITE_APP_LANG=en 留在本地开发");
  }
}

function checkUiSource() {
  const config = read("app/src/lib/config.ts");
  if (!config.includes("VITE_APP_NAME") && !config.includes(ZH_DISPLAY_NAME)) {
    fail("config.ts 必须能得到中文显示名「御钥师」");
  }
  if (!config.includes(ZH_DISPLAY_NAME) || !config.includes(EN_DISPLAY_NAME)) {
    fail(`config.ts 回退显示名必须与 brand.mjs 一致（${ZH_DISPLAY_NAME} / ${EN_DISPLAY_NAME}）`);
  }

  const titleBar = read("app/src/ui/TitleBar.tsx");
  if (titleBar.includes("getName") || titleBar.includes("@tauri-apps/api/app")) {
    fail("TitleBar 禁止调用 getName() 或引用 @tauri-apps/api/app。左上角只渲染 {APP_NAME}");
  }
  if (!titleBar.includes("{APP_NAME}")) {
    fail("TitleBar 左上角必须直接渲染 {APP_NAME}，不要用 productName / 运行时窗口名覆盖");
  }

  const mobile = read("app/src/ui/MobileShell.tsx");
  if (mobile.includes("getName")) {
    fail("MobileShell 禁止调用 getName()");
  }
  if (!mobile.includes("{APP_NAME}")) {
    fail("MobileShell 顶栏必须渲染 {APP_NAME}");
  }

  const vite = read("app/vite.config.ts");
  if (!vite.includes("from '../scripts/brand.mjs'") && !vite.includes('from "../scripts/brand.mjs"')) {
    fail("vite.config.ts 必须从 scripts/brand.mjs 注入显示名，禁止前端再抄一份");
  }
  if (!vite.includes("VITE_APP_NAME")) {
    fail("vite.config.ts 必须 define import.meta.env.VITE_APP_NAME");
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
