export const APP_LANG = (import.meta.env.VITE_APP_LANG || "zh") as "zh" | "en";
export const APP_NAME = APP_LANG === "en" ? "Git Keymaster" : "御钥师";
