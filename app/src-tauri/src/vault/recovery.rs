//! 数据恢复密钥：256-bit 随机 → Crockford Base32 + 版本前缀 + 校验，分组展示。
//!
//! 形态示例：`GAM1-K7X2-9MQR-4TWB-...-H3N8`
//! - 大小写不敏感；自动忽略连字符与空格；I/L→1、O→0 混淆纠正。
//! - 末尾带 2 字节校验（SHA256 前缀），抄错能被立即发现。

use crate::error::{AppError, Result};
use crate::vault::crypto::{fill_random, KEY_LEN};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

const PREFIX: &str = "GAM1";
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CHECKSUM_LEN: usize = 2;

/// 恢复密钥的原始秘密（32 字节），Drop 清零。
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct RecoverySecret(pub [u8; KEY_LEN]);

impl RecoverySecret {
    pub fn random() -> Self {
        let mut b = [0u8; KEY_LEN];
        fill_random(&mut b);
        RecoverySecret(b)
    }
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

fn checksum(secret: &[u8]) -> [u8; CHECKSUM_LEN] {
    let digest = Sha256::digest(secret);
    let mut c = [0u8; CHECKSUM_LEN];
    c.copy_from_slice(&digest[..CHECKSUM_LEN]);
    c
}

/// 生成一把恢复密钥，返回 (秘密, 展示字符串)。
pub fn generate() -> (RecoverySecret, String) {
    let secret = RecoverySecret::random();
    let display = format_display(&secret);
    (secret, display)
}

/// 把秘密编码成分组展示字符串。
pub fn format_display(secret: &RecoverySecret) -> String {
    let mut payload = Vec::with_capacity(KEY_LEN + CHECKSUM_LEN);
    payload.extend_from_slice(secret.as_bytes());
    payload.extend_from_slice(&checksum(secret.as_bytes()));
    let body = b32_encode(&payload);
    let raw = format!("{}{}", PREFIX, body);
    // 每 4 个字符一组，用连字符连接。
    raw.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).to_string())
        .collect::<Vec<_>>()
        .join("-")
}

/// 解析并校验用户输入的恢复密钥，成功返回 32 字节秘密。
pub fn parse(input: &str) -> Result<RecoverySecret> {
    let cleaned = normalize(input);
    if !cleaned.starts_with(PREFIX) {
        return Err(AppError::BadRecoveryKey("缺少或错误的版本前缀".into()));
    }
    let body = &cleaned[PREFIX.len()..];
    let decoded = b32_decode(body)
        .map_err(|_| AppError::BadRecoveryKey("包含无法识别的字符".into()))?;
    if decoded.len() < KEY_LEN + CHECKSUM_LEN {
        return Err(AppError::BadRecoveryKey("长度不足".into()));
    }
    let (secret_bytes, rest) = decoded.split_at(KEY_LEN);
    let got = &rest[..CHECKSUM_LEN];
    let expect = checksum(secret_bytes);
    if got != expect {
        return Err(AppError::BadRecoveryKey("校验未通过，可能抄写有误".into()));
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(secret_bytes);
    Ok(RecoverySecret(arr))
}

/// 规范化：大写、去连字符/空白、混淆字符纠正。
fn normalize(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            other => other,
        })
        .collect()
}

fn b32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for &b in data {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let idx = ((buffer >> bits) & 0x1f) as usize;
            out.push(ALPHABET[idx] as char);
        }
    }
    if bits > 0 {
        let idx = ((buffer << (5 - bits)) & 0x1f) as usize;
        out.push(ALPHABET[idx] as char);
    }
    out
}

fn char_value(c: char) -> std::result::Result<u8, ()> {
    ALPHABET
        .iter()
        .position(|&a| a as char == c)
        .map(|p| p as u8)
        .ok_or(())
}

fn b32_decode(s: &str) -> std::result::Result<Vec<u8>, ()> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for c in s.chars() {
        let v = char_value(c)?;
        buffer = (buffer << 5) | v as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_generate_parse() {
        let (secret, display) = generate();
        let parsed = parse(&display).unwrap();
        assert_eq!(parsed.as_bytes(), secret.as_bytes());
    }

    #[test]
    fn display_has_prefix_and_groups() {
        let (_s, d) = generate();
        assert!(d.starts_with("GAM1-"));
        for group in d.split('-') {
            assert!(group.len() <= 4 && !group.is_empty());
        }
    }

    #[test]
    fn case_and_separator_insensitive() {
        let (secret, display) = generate();
        let messy = format!("  {}  ", display.to_lowercase().replace('-', " "));
        let parsed = parse(&messy).unwrap();
        assert_eq!(parsed.as_bytes(), secret.as_bytes());
    }

    #[test]
    fn confusable_chars_corrected() {
        // 用 known 秘密构造展示串，再把其中的 1/0 替换成 l/O 输入。
        let secret = RecoverySecret([0xABu8; KEY_LEN]);
        let display = format_display(&secret);
        // 人为把可能出现的数字用混淆字母替换（仅当存在时）。
        let messy = display.replace('1', "l").replace('0', "O");
        let parsed = parse(&messy).unwrap();
        assert_eq!(parsed.as_bytes(), secret.as_bytes());
    }

    #[test]
    fn tampered_key_fails_checksum() {
        let (_s, display) = generate();
        let mut chars: Vec<char> = display.chars().filter(|c| *c != '-').collect();
        // 改动前缀之后的第一个数据字符。
        let idx = PREFIX.len();
        chars[idx] = if chars[idx] == 'Z' { 'Y' } else { 'Z' };
        let tampered: String = chars.into_iter().collect();
        assert!(parse(&tampered).is_err());
    }

    #[test]
    fn wrong_prefix_rejected() {
        assert!(parse("XXXX-K7X2-9MQR").is_err());
    }
}
