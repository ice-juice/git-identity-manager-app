import { api, type ClipboardWriteResult } from "./ipc";

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const PLAIN: ClipboardWriteResult = { excluded: false, fallback: false };

/** 走系统剪贴板，避免 WebView 弹出 localhost 权限框。机密路径传 `secret: true`。 */
export async function writeClipboard(text: string, secret = false): Promise<ClipboardWriteResult> {
  if (isTauri()) {
    const result = await api.clipboardWrite(text, secret);
    if (result.notice) {
      console.warn(result.notice);
    }
    return result;
  }
  await navigator.clipboard.writeText(text);
  return PLAIN;
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
