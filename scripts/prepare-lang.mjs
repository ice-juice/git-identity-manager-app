import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

const lang = (process.argv[2] || "zh").toLowerCase();

const tauriConfPath = path.join(rootDir, "app", "src-tauri", "tauri.conf.json");
const tauriConf = JSON.parse(fs.readFileSync(tauriConfPath, "utf-8"));

if (lang === "en") {
  tauriConf.productName = "Git Keymaster";
  if (tauriConf.app && tauriConf.app.windows && tauriConf.app.windows[0]) {
    tauriConf.app.windows[0].title = "Git Keymaster";
  }
} else {
  tauriConf.productName = "御钥师";
  if (tauriConf.app && tauriConf.app.windows && tauriConf.app.windows[0]) {
    tauriConf.app.windows[0].title = "御钥师";
  }
}

fs.writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2), "utf-8");

const envPath = path.join(rootDir, "app", ".env");
fs.writeFileSync(envPath, `VITE_APP_LANG=${lang}\n`, "utf-8");

console.log(`[prepare-lang] Applied language configuration: ${lang} (productName: ${tauriConf.productName})`);
