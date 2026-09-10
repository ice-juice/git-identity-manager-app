//! RFC 6238 TOTP 生成与 otpauth URI 解析/拼回。

use crate::error::{AppError, Result};
use crate::model::TotpEntry;
use data_encoding::BASE32;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use url::Url;

pub fn decode_secret(raw: &str) -> Result<Vec<u8>> {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if cleaned.is_empty() {
        return Err(AppError::Invalid("TOTP 密钥为空".into()));
    }
    let padded = match cleaned.len() % 8 {
        0 => cleaned,
        r => format!("{}{}", cleaned, "=".repeat(8 - r)),
    };
    BASE32
        .decode(padded.as_bytes())
        .map_err(|_| AppError::Invalid("TOTP 密钥不是有效的 Base32".into()))
}

pub fn normalize_secret(raw: &str) -> Result<String> {
    let bytes = decode_secret(raw)?;
    Ok(BASE32.encode(&bytes).trim_end_matches('=').to_string())
}

pub fn generate_code(secret: &str, algorithm: &str, digits: u8, period: u32, unix_secs: u64) -> Result<String> {
    let key = decode_secret(secret)?;
    let period = period.max(1) as u64;
    let digits = digits.clamp(6, 8);
    let counter = unix_secs / period;
    let mut msg = [0u8; 8];
    msg.copy_from_slice(&counter.to_be_bytes());
    let hs = hmac_digest(algorithm, &key, &msg)?;
    let offset = (hs[hs.len() - 1] & 0x0f) as usize;
    let bin = ((u32::from(hs[offset]) & 0x7f) << 24)
        | (u32::from(hs[offset + 1]) << 16)
        | (u32::from(hs[offset + 2]) << 8)
        | u32::from(hs[offset + 3]);
    let modulus = 10u32.pow(u32::from(digits));
    Ok(format!("{:0width$}", bin % modulus, width = digits as usize))
}

pub fn remaining_seconds(period: u32, unix_secs: u64) -> u32 {
    let period = period.max(1) as u64;
    (period - (unix_secs % period)) as u32
}

fn hmac_digest(algorithm: &str, key: &[u8], msg: &[u8]) -> Result<Vec<u8>> {
    match algorithm.to_ascii_uppercase().as_str() {
        "SHA256" => {
            let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| AppError::Crypto)?;
            mac.update(msg);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "SHA512" => {
            let mut mac = Hmac::<Sha512>::new_from_slice(key).map_err(|_| AppError::Crypto)?;
            mac.update(msg);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "SHA1" | "" => {
            let mut mac = Hmac::<Sha1>::new_from_slice(key).map_err(|_| AppError::Crypto)?;
            mac.update(msg);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        other => Err(AppError::Invalid(format!("不支持的 TOTP 算法：{other}"))),
    }
}

#[derive(Debug, Clone)]
pub struct ParsedOtpauth {
    pub issuer: String,
    pub account: String,
    pub secret: String,
    pub algorithm: String,
    pub digits: u8,
    pub period: u32,
}

pub fn parse_otpauth(uri: &str) -> Result<ParsedOtpauth> {
    let trimmed = uri.trim();
    if !trimmed.to_ascii_lowercase().starts_with("otpauth://totp/") {
        return Err(AppError::Invalid("只支持 otpauth://totp/ 链接".into()));
    }
    let url = Url::parse(trimmed).map_err(|_| AppError::Invalid("otpauth 链接格式无效".into()))?;
    let mut pairs = std::collections::HashMap::new();
    for (k, v) in url.query_pairs() {
        pairs.insert(k.to_ascii_lowercase(), v.to_string());
    }
    let secret = pairs
        .get("secret")
        .cloned()
        .ok_or_else(|| AppError::Invalid("otpauth 缺少 secret".into()))?;
    let secret = normalize_secret(&secret)?;

    let label = url
        .path_segments()
        .and_then(|mut s| s.next())
        .unwrap_or("")
        .to_string();
    let label = percent_decode(&label);
    let (label_issuer, account) = if let Some((a, b)) = label.split_once(':') {
        (a.trim().to_string(), b.trim().to_string())
    } else if let Some((a, b)) = label.split_once('%') {
        let _ = (a, b);
        (String::new(), label)
    } else {
        (String::new(), label)
    };
    let issuer = pairs
        .get("issuer")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(label_issuer);

    let algorithm = pairs
        .get("algorithm")
        .map(|s| s.to_ascii_uppercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "SHA1".into());
    let digits = pairs
        .get("digits")
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(6)
        .clamp(6, 8);
    let period = pairs
        .get("period")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(30)
        .max(1);

    if account.is_empty() && issuer.is_empty() {
        return Err(AppError::Invalid("otpauth 缺少账号标识".into()));
    }

    Ok(ParsedOtpauth {
        issuer: if issuer.is_empty() { "未命名".into() } else { issuer },
        account: if account.is_empty() { "default".into() } else { account },
        secret,
        algorithm,
        digits,
        period,
    })
}

pub fn build_otpauth(entry: &TotpEntry, secret: &str) -> String {
    let label = format!("{}:{}", encode_component(&entry.issuer), encode_component(&entry.account));
    format!(
        "otpauth://totp/{label}?secret={}&issuer={}&algorithm={}&digits={}&period={}",
        secret,
        encode_component(&entry.issuer),
        entry.algorithm,
        entry.digits,
        entry.period
    )
}

fn encode_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_sha1_8digits() {
        // secret ASCII "12345678901234567890" = GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ
        let secret = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        assert_eq!(generate_code(secret, "SHA1", 8, 30, 59).unwrap(), "94287082");
        assert_eq!(generate_code(secret, "SHA1", 8, 30, 1_111_111_109).unwrap(), "07081804");
    }

    #[test]
    fn otpauth_roundtrip() {
        let parsed = parse_otpauth(
            "otpauth://totp/GitHub:techn4950?secret=JBSWY3DPEHPK3PXP&issuer=GitHub&algorithm=SHA1&digits=6&period=30",
        )
        .unwrap();
        assert_eq!(parsed.issuer, "GitHub");
        assert_eq!(parsed.account, "techn4950");
        let entry = TotpEntry {
            id: "x".into(),
            issuer: parsed.issuer.clone(),
            account: parsed.account.clone(),
            note: None,
            url: None,
            group: None,
            algorithm: parsed.algorithm.clone(),
            digits: parsed.digits,
            period: parsed.period,
            icon: None,
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let uri = build_otpauth(&entry, &parsed.secret);
        let again = parse_otpauth(&uri).unwrap();
        assert_eq!(again.secret, parsed.secret);
        assert_eq!(again.issuer, parsed.issuer);
        assert_eq!(again.account, parsed.account);
    }
}
