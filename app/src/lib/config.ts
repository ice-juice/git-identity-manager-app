function parseAppLang(raw: unknown): "zh" | "en" {
  const s = String(raw ?? "zh").trim().toLowerCase();
  if (s === "en" || s.startsWith("en-")) return "en";
  return "zh";
}

export const APP_LANG = parseAppLang(import.meta.env.VITE_APP_LANG);

/** 编译期注入的显示名。左上角 / 移动端顶栏只渲染这个，禁止再用 getName() 覆盖。 */
export const APP_NAME =
  String(import.meta.env.VITE_APP_NAME || "").trim() ||
  (APP_LANG === "en" ? "Git Keymaster" : "御钥师");
