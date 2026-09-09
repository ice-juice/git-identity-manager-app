export type ThemeMode = "light" | "dark" | "navy";

export interface ThemeOption {
  id: ThemeMode;
  name: string;
  desc: string;
  previewBg: string;
  previewCard: string;
  previewAccent: string;
}

export const THEME_OPTIONS: ThemeOption[] = [
  {
    id: "light",
    name: "经典蓝紫（默认）",
    desc: "深邃侧边栏 + 亮白卡片 + 蓝紫高光，经典耐看",
    previewBg: "#0f172a",
    previewCard: "#ffffff",
    previewAccent: "#4f46e5",
  },
  {
    id: "dark",
    name: "冷萃深色",
    desc: "内敛低饱和、柔和炭灰、夜间护眼不刺眼",
    previewBg: "#12151c",
    previewCard: "#181d28",
    previewAccent: "#6366f1",
  },
  {
    id: "navy",
    name: "沉稳黛蓝",
    desc: "商务蓝调深色、专业典雅",
    previewBg: "#060912",
    previewCard: "#0e1628",
    previewAccent: "#0284c7",
  },
];

const STORAGE_KEY = "gam.theme";

export function getSavedTheme(): ThemeMode {
  const saved = localStorage.getItem(STORAGE_KEY) as ThemeMode | null;
  if (saved && (saved === "light" || saved === "dark" || saved === "navy")) {
    return saved;
  }
  return "light";
}

export function applyTheme(theme: ThemeMode) {
  document.documentElement.setAttribute("data-theme", theme);
  localStorage.setItem(STORAGE_KEY, theme);
}
