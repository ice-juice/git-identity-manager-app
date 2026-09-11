import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

const lang = (process.argv[2] || "zh").toLowerCase();
const locale = lang === "en" ? "en-US" : "zh-CN";

function upsertPlistString(xml, key, value) {
  const re = new RegExp(`(<key>${key}</key>\\s*<string>)([^<]*)(</string>)`);
  if (re.test(xml)) {
    return xml.replace(re, `$1${value}$3`);
  }
  return xml.replace("</dict>", `\t<key>${key}</key>\n\t<string>${value}</string>\n</dict>`);
}

const tauriConfPath = path.join(rootDir, "app", "src-tauri", "tauri.conf.json");
const tauriConf = JSON.parse(fs.readFileSync(tauriConfPath, "utf-8"));

// 安装包文件名由 rename-release-assets.mjs 统一成 ASCII 的 Git.Keymaster_*。
// productName 仍是桌面快捷方式、开始菜单、卸载项上的显示名（中文包为御钥师）。
// Windows 默认安装目录由 windows/installer.nsi 固定为 GitKeymaster，与显示名拆开。
tauriConf.mainBinaryName = "git-keymaster";
const displayName = lang === "en" ? "Git Keymaster" : "御钥师";
tauriConf.productName = displayName;
if (tauriConf.app && tauriConf.app.windows && tauriConf.app.windows[0]) {
  tauriConf.app.windows[0].title = displayName;
}

const infoPlistPath = path.join(rootDir, "app", "src-tauri", "Info.plist");
if (fs.existsSync(infoPlistPath)) {
  let plist = fs.readFileSync(infoPlistPath, "utf-8");
  plist = upsertPlistString(plist, "CFBundleDisplayName", displayName);
  // 程序坞显示用 DisplayName；短名保持 ASCII，避免部分工具按文件夹名解析失败。
  plist = upsertPlistString(plist, "CFBundleName", "GitKeymaster");
  fs.writeFileSync(infoPlistPath, plist, "utf-8");
}

tauriConf.bundle = tauriConf.bundle || {};
tauriConf.bundle.targets = ["nsis", "msi", "app", "dmg", "appimage", "deb", "rpm"];
tauriConf.bundle.windows = tauriConf.bundle.windows || {};
tauriConf.bundle.windows.nsis = {
  ...(tauriConf.bundle.windows.nsis || {}),
  displayLanguageSelector: false,
  languages: [lang === "en" ? "English" : "SimpChinese"],
  template: "windows/installer.nsi",
};
tauriConf.bundle.windows.wix = {
  ...(tauriConf.bundle.windows.wix || {}),
  language: locale,
};

fs.writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2), "utf-8");

const envPath = path.join(rootDir, "app", ".env");
fs.writeFileSync(envPath, `VITE_APP_LANG=${lang}\n`, "utf-8");

console.log(`[prepare-lang] ${lang} locale=${locale} productName=${tauriConf.productName} title=${tauriConf.app.windows[0].title}`);
