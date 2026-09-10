import { api } from "./ipc";

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 走系统剪贴板，避免 WebView 弹出 localhost 权限框。 */
export async function writeClipboard(text: string) {
  if (isTauri()) {
    await api.clipboardWrite(text);
    return;
  }
  await navigator.clipboard.writeText(text);
}

export async function clearClipboard() {
  if (isTauri()) {
    await api.clipboardClear();
    return;
  }
  try {
    await navigator.clipboard.writeText("");
  } catch {
    /* ignore */
  }
}
