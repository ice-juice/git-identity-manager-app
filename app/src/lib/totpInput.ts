/** 识别用户粘贴的是 otpauth 链接还是 Base32 密钥（二者只需填一种）。 */

export type TotpSecretKind = "empty" | "otpauth" | "base32" | "unknown";

export interface DetectedTotpInput {
  kind: TotpSecretKind;
  raw: string;
  secret?: string;
  issuer?: string;
  account?: string;
  algorithm?: string;
  digits?: number;
  period?: number;
  message: string;
}

export function detectTotpInput(raw: string): DetectedTotpInput {
  const trimmed = raw.trim();
  if (!trimmed) {
    return { kind: "empty", raw, message: "从网站两步验证页复制密钥或 otpauth 链接，粘贴即可。" };
  }

  const uriStart = trimmed.toLowerCase().indexOf("otpauth://");
  if (uriStart >= 0) {
    const uri = trimmed.slice(uriStart).split(/\s/)[0];
    return parseOtpauthClient(uri, raw);
  }

  const maybeLabeled = trimmed.replace(/^secret\s*=\s*/i, "").trim();
  const compact = maybeLabeled.replace(/[\s\-]/g, "").toUpperCase();
  if (/^[A-Z2-7]+=*$/.test(compact) && compact.replace(/=+$/, "").length >= 8) {
    return {
      kind: "base32",
      raw,
      secret: compact,
      message: "已识别为 Base32 密钥。再补一下平台和账号即可保存。",
    };
  }

  return {
    kind: "unknown",
    raw,
    message: "无法识别。请粘贴 otpauth://totp/... 链接，或一串 Base32 密钥（如 JBSW Y3DP EHPK 3PXP）。",
  };
}

function parseOtpauthClient(uri: string, raw: string): DetectedTotpInput {
  if (!uri.toLowerCase().startsWith("otpauth://totp/")) {
    return { kind: "unknown", raw, message: "目前只支持 otpauth://totp/ 链接。" };
  }
  try {
    const u = new URL(uri);
    const secretRaw = u.searchParams.get("secret") || "";
    const compact = secretRaw.replace(/[\s\-]/g, "").toUpperCase();
    if (!compact) {
      return { kind: "unknown", raw, message: "otpauth 链接里没有 secret。" };
    }
    const label = decodeURIComponent((u.pathname || "").replace(/^\//, ""));
    let labelIssuer = "";
    let account = label;
    const colon = label.indexOf(":");
    if (colon >= 0) {
      labelIssuer = label.slice(0, colon).trim();
      account = label.slice(colon + 1).trim();
    }
    const issuer = (u.searchParams.get("issuer") || labelIssuer).trim() || "未命名";
    const algorithm = (u.searchParams.get("algorithm") || "SHA1").toUpperCase();
    const digits = clamp(Number(u.searchParams.get("digits") || 6), 6, 8);
    const period = Math.max(1, Number(u.searchParams.get("period") || 30) || 30);
    return {
      kind: "otpauth",
      raw,
      secret: compact,
      issuer,
      account: account || "default",
      algorithm,
      digits,
      period,
      message: `已识别为 otpauth 链接，已填入 ${issuer} / ${account || "账号"}。`,
    };
  } catch {
    return { kind: "unknown", raw, message: "otpauth 链接格式无效。" };
  }
}

function clamp(n: number, min: number, max: number) {
  if (Number.isNaN(n)) return min;
  return Math.min(max, Math.max(min, n));
}
