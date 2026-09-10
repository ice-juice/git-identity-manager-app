/** 识别账号表单里粘贴的是网址还是平台名，并尽量回填平台 / URL / 图标。 */

export interface BuiltinHint {
  id: string;
  name: string;
}

export interface DetectedAccountSource {
  kind: "empty" | "url" | "name";
  platform: string;
  url?: string;
  icon?: string;
  message: string;
}

export function detectAccountSource(raw: string, builtins: BuiltinHint[]): DetectedAccountSource {
  const trimmed = raw.trim();
  if (!trimmed) {
    return { kind: "empty", platform: "", message: "填网站名，或直接粘贴登录页网址。" };
  }

  if (looksLikeUrl(trimmed)) {
    const url = normalizeUrl(trimmed);
    let host = "";
    try {
      host = new URL(url).hostname.replace(/^www\./i, "");
    } catch {
      host = trimmed;
    }
    const icon = suggestIcon(host, builtins);
    const platform = friendlyPlatform(host, builtins);
    return {
      kind: "url",
      platform,
      url,
      icon,
      message: `已识别网址，平台填为「${platform}」。`,
    };
  }

  const icon = suggestIcon(trimmed, builtins);
  return {
    kind: "name",
    platform: trimmed,
    icon,
    message: icon ? `将使用「${builtins.find((b) => `builtin:${b.id}` === icon)?.name}」图标。` : "保存时会按平台名自动匹配图标。",
  };
}

export function suggestIcon(name: string, builtins: BuiltinHint[]): string | undefined {
  const q = name.trim().toLowerCase();
  if (!q) return undefined;
  const hit = builtins.find(
    (i) => q.includes(i.id) || i.name.toLowerCase().includes(q) || q.includes(i.name.toLowerCase()),
  );
  return hit ? `builtin:${hit.id}` : undefined;
}

function looksLikeUrl(s: string): boolean {
  if (/^https?:\/\//i.test(s)) return true;
  return /^[\w.-]+\.[a-z]{2,}([/:?#].*)?$/i.test(s);
}

function normalizeUrl(s: string): string {
  if (/^https?:\/\//i.test(s)) return s;
  return `https://${s}`;
}

function friendlyPlatform(host: string, builtins: BuiltinHint[]): string {
  const icon = suggestIcon(host, builtins);
  if (icon) {
    const id = icon.slice("builtin:".length);
    return builtins.find((b) => b.id === id)?.name || host;
  }
  const head = host.split(".")[0] || host;
  return head.charAt(0).toUpperCase() + head.slice(1);
}
