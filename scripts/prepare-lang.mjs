import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

const lang = (process.argv[2] || "zh").toLowerCase();
const locale = lang === "en" ? "en-US" : "zh-CN";

const tauriConfPath = path.join(rootDir, "app", "src-tauri", "tauri.conf.json");
const tauriConf = JSON.parse(fs.readFileSync(tauriConfPath, "utf-8"));

// 安装包文件名必须用 ASCII。界面标题仍按语言区分。
tauriConf.mainBinaryName = "git-account-manager";
tauriConf.productName = "Git.Keymaster";
if (tauriConf.app && tauriConf.app.windows && tauriConf.app.windows[0]) {
  tauriConf.app.windows[0].title = lang === "en" ? "Git Keymaster" : "御钥师";
}

tauriConf.bundle = tauriConf.bundle || {};
tauriConf.bundle.targets = ["nsis", "msi", "app", "dmg", "appimage", "deb", "rpm"];
tauriConf.bundle.windows = tauriConf.bundle.windows || {};
tauriConf.bundle.windows.nsis = {
  ...(tauriConf.bundle.windows.nsis || {}),
  displayLanguageSelector: false,
  languages: [lang === "en" ? "English" : "SimpChinese"],
};
tauriConf.bundle.windows.wix = {
  ...(tauriConf.bundle.windows.wix || {}),
  language: locale,
};

fs.writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2), "utf-8");

const envPath = path.join(rootDir, "app", ".env");
fs.writeFileSync(envPath, `VITE_APP_LANG=${lang}\n`, "utf-8");

console.log(`[prepare-lang] ${lang} locale=${locale} productName=${tauriConf.productName} title=${tauriConf.app.windows[0].title}`);
