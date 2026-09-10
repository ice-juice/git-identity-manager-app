import { api, errCode, errMessage } from "./ipc";
import { clearClipboard, writeClipboard } from "./clipboard";

const customCache = new Map<string, string>();
let clearTimer: number | null = null;

export async function copyWithClear(text: string, seconds?: number, secret = true) {
  const result = await writeClipboard(text, secret);
  let wait = seconds;
  if (wait === undefined) {
    try {
      wait = (await api.getRevealSettings()).clipboardClearSeconds;
    } catch {
      wait = 20;
    }
  }
  if (clearTimer !== null) {
    window.clearTimeout(clearTimer);
    clearTimer = null;
  }
  if (wait && wait > 0) {
    clearTimer = window.setTimeout(() => {
      clearTimer = null;
      clearClipboard().catch(() => {});
    }, wait * 1000);
  }
  return result;
}

export function isNeedReauth(e: unknown): boolean {
  return errCode(e) === "NEED_REAUTH" || errMessage(e).includes("访问密码");
}

export async function customIconUrl(iconRef: string | null | undefined): Promise<string | null> {
  if (!iconRef?.startsWith("custom:")) return null;
  const hit = customCache.get(iconRef);
  if (hit) return hit;
  const url = await api.iconGetCustom(iconRef);
  customCache.set(iconRef, url);
  return url;
}
