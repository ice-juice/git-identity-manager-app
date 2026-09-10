// 应用级 UI 偏好（纯本地，无需后端）——与 theme.ts 同风格。

const UNLOCK_ANIM_KEY = "gam.unlockAnim";

/** 解锁开门动画是否开启，默认开启。 */
export function getUnlockAnimEnabled(): boolean {
  return localStorage.getItem(UNLOCK_ANIM_KEY) !== "off";
}

export function setUnlockAnimEnabledStored(on: boolean): void {
  localStorage.setItem(UNLOCK_ANIM_KEY, on ? "on" : "off");
}

/** 系统是否开启了「减少动态效果」。开启时应跳过过场动画。 */
export function prefersReducedMotion(): boolean {
  try {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    return false;
  }
}
