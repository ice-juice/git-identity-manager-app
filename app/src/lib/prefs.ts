// 应用级 UI 偏好（纯本地，无需后端）——与 theme.ts 同风格。

const UNLOCK_ANIM_KEY = "gam.unlockAnim";
const UNLOCK_ANIM_STYLE_KEY = "gam.unlockAnimStyle";

export type UnlockAnimStyle = "cyber" | "classic" | "minimal";

export const UNLOCK_ANIM_STYLES: { id: UnlockAnimStyle; label: string; desc: string; icon: string }[] = [
  {
    id: "cyber",
    label: "赛博全息",
    desc: "全息猫扫描 · 激光能量钥 · 气密舱门",
    icon: "⚡",
  },
  {
    id: "classic",
    label: "经典金匙",
    desc: "吉祥猫 · 黄金实体钥匙 · 暖光大门旋开",
    icon: "🗝️",
  },
  {
    id: "minimal",
    label: "极客量子",
    desc: "量子核心锁 · 极速代码流 · 瞬间破壁",
    icon: "💻",
  },
];

/** 解锁开门动画是否开启，默认开启。 */
export function getUnlockAnimEnabled(): boolean {
  return localStorage.getItem(UNLOCK_ANIM_KEY) !== "off";
}

export function setUnlockAnimEnabledStored(on: boolean): void {
  localStorage.setItem(UNLOCK_ANIM_KEY, on ? "on" : "off");
}

/** 获取解锁动画风格，默认赛博全息。 */
export function getUnlockAnimStyle(): UnlockAnimStyle {
  const s = localStorage.getItem(UNLOCK_ANIM_STYLE_KEY);
  if (s === "classic" || s === "minimal" || s === "cyber") return s;
  return "cyber";
}

export function setUnlockAnimStyleStored(style: UnlockAnimStyle): void {
  localStorage.setItem(UNLOCK_ANIM_STYLE_KEY, style);
}

/** 系统是否开启了「减少动态效果」。开启时应跳过过场动画。 */
export function prefersReducedMotion(): boolean {
  try {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    return false;
  }
}
