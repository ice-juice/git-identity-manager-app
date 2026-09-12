/**
 * 品牌名与标识符的唯一来源。显示名跟语言走；路径 / 文件名 / 主程序保持 ASCII。
 * 改名字只改这里，再跑 `node scripts/check-release-invariants.mjs --test`。
 */
export const ZH_DISPLAY_NAME = "御钥师";
export const EN_DISPLAY_NAME = "Git Keymaster";
export const MAIN_BINARY = "git-keymaster";
export const WIN_INSTALL_DIR = "GitKeymaster";
export const INSTALLER_STEM = "Git.Keymaster";

export function isEnglishLang(lang) {
  const s = String(lang ?? "zh").trim().toLowerCase();
  return s === "en" || s.startsWith("en-") || s === "en-us";
}

export function displayNameFor(lang) {
  return isEnglishLang(lang) ? EN_DISPLAY_NAME : ZH_DISPLAY_NAME;
}
