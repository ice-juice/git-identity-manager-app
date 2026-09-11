function parseAppLang(raw: unknown): "zh" | "en" {
  const s = String(raw ?? "zh").trim().toLowerCase();
  if (s === "en" || s.startsWith("en-")) return "en";
  return "zh";
}

export const APP_LANG = parseAppLang(import.meta.env.VITE_APP_LANG);
export const APP_NAME = APP_LANG === "en" ? "Git Keymaster" : "御钥师";

/** 中文包左上角必须是「御钥师」。运行时名或语言任一为中文即用中文名。 */
export function pickBrandName(lang: "zh" | "en", runtimeName?: string | null): string {
  const runtime = (runtimeName ?? "").trim();
  if (runtime.includes("御钥师")) return "御钥师";
  if (lang === "zh") return "御钥师";
  return runtime || "Git Keymaster";
}
