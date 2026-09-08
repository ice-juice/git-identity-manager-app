//! SSH 密钥解析：类型、指纹、有无口令、公钥导出、公私钥配对校验、格式检测。
//! 纯 Rust（`ssh-key`），不依赖外部命令。

use crate::error::{AppError, Result};
use serde::Serialize;
use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey, PublicKey};
use zeroize::Zeroizing;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    /// 友好类型：ed25519 / rsa / ecdsa / dsa / 其他。
    pub algorithm: String,
    /// SHA256 指纹（形如 `SHA256:...`）。
    pub fingerprint: String,
    /// OpenSSH 单行公钥。
    pub public_openssh: String,
    /// 位数（RSA 有意义；ed25519 固定 256）。
    pub bits: Option<u32>,
    /// 私钥是否带口令（无私钥时为 None）。
    pub encrypted: Option<bool>,
    pub comment: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyFormat {
    OpenSsh,
    /// 传统 PEM（RSA/EC），建议转 OpenSSH 新格式。
    Pem,
    /// PuTTY .ppk，OpenSSH 不识别，需转换。
    Ppk,
    Unknown,
}

pub fn detect_format(text: &str) -> KeyFormat {
    let t = text.trim_start();
    if t.starts_with("PuTTY-User-Key-File") {
        KeyFormat::Ppk
    } else if t.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----") {
        KeyFormat::OpenSsh
    } else if t.starts_with("-----BEGIN") && t.contains("PRIVATE KEY-----") {
        KeyFormat::Pem
    } else if t.starts_with("ssh-") || t.starts_with("ecdsa-") || t.starts_with("sk-") {
        KeyFormat::OpenSsh // 公钥单行
    } else {
        KeyFormat::Unknown
    }
}

fn friendly_alg(a: &Algorithm) -> String {
    match a {
        Algorithm::Ed25519 => "ed25519".into(),
        Algorithm::Rsa { .. } => "rsa".into(),
        Algorithm::Ecdsa { .. } => "ecdsa".into(),
        Algorithm::Dsa => "dsa".into(),
        other => other.as_str().to_string(),
    }
}

fn bits_of(pk: &PublicKey) -> Option<u32> {
    use ssh_key::public::KeyData;
    match pk.key_data() {
        KeyData::Ed25519(_) => Some(256),
        KeyData::Rsa(rsa) => {
            // 模数字节长度 → 位数；去掉可能的符号前导零。
            let n = rsa.n.as_bytes();
            let mut len = n.len();
            if len > 0 && n[0] == 0 {
                len -= 1;
            }
            Some((len * 8) as u32)
        }
        KeyData::Ecdsa(e) => Some(match e.curve() {
            ssh_key::EcdsaCurve::NistP256 => 256,
            ssh_key::EcdsaCurve::NistP384 => 384,
            ssh_key::EcdsaCurve::NistP521 => 521,
        }),
        _ => None,
    }
}

fn info_from_public(pk: &PublicKey, encrypted: Option<bool>) -> KeyInfo {
    KeyInfo {
        algorithm: friendly_alg(&pk.algorithm()),
        fingerprint: pk.fingerprint(HashAlg::Sha256).to_string(),
        public_openssh: pk.to_openssh().unwrap_or_default(),
        bits: bits_of(pk),
        encrypted,
        comment: pk.comment().to_string(),
    }
}

/// 解析单行 OpenSSH 公钥。
pub fn parse_public_openssh(text: &str) -> Result<KeyInfo> {
    let pk = PublicKey::from_openssh(text.trim())
        .map_err(|e| AppError::Invalid(format!("公钥解析失败：{e}")))?;
    Ok(info_from_public(&pk, None))
}

/// 解析 OpenSSH 私钥（即便带口令也能取出公钥/指纹，无需解密）。
pub fn parse_private_openssh(text: &str) -> Result<KeyInfo> {
    let sk = PrivateKey::from_openssh(text)
        .map_err(|e| AppError::Invalid(format!("私钥解析失败：{e}")))?;
    let pk = sk.public_key();
    Ok(info_from_public(pk, Some(sk.is_encrypted())))
}

/// 校验公私钥是否配对（比较指纹）。
pub fn pair_matches(private_openssh: &str, public_openssh: &str) -> Result<bool> {
    let sk = PrivateKey::from_openssh(private_openssh)
        .map_err(|e| AppError::Invalid(format!("私钥解析失败：{e}")))?;
    let pk = PublicKey::from_openssh(public_openssh.trim())
        .map_err(|e| AppError::Invalid(format!("公钥解析失败：{e}")))?;
    let f1 = sk.public_key().fingerprint(HashAlg::Sha256);
    let f2 = pk.fingerprint(HashAlg::Sha256);
    Ok(f1 == f2)
}

/// 校验口令是否正确（尝试解密）。
pub fn verify_passphrase(private_openssh: &str, passphrase: &str) -> Result<()> {
    let sk = PrivateKey::from_openssh(private_openssh)
        .map_err(|e| AppError::Invalid(format!("私钥解析失败：{e}")))?;
    if !sk.is_encrypted() {
        return Ok(());
    }
    sk.decrypt(passphrase)
        .map(|_| ())
        .map_err(|_| AppError::BadPassword)
}

/// 解密私钥为**不带口令**的 OpenSSH 文本，用于 `ssh-add -` 从 stdin 加载
/// （严格模式：私钥明文全程不落盘）。返回值 Drop 时清零。
pub fn decrypt_to_openssh(private_openssh: &str, passphrase: Option<&str>) -> Result<Zeroizing<String>> {
    let sk = PrivateKey::from_openssh(private_openssh)
        .map_err(|e| AppError::Invalid(format!("私钥解析失败：{e}")))?;
    let decrypted = if sk.is_encrypted() {
        let pw = passphrase.ok_or_else(|| AppError::Invalid("该私钥带口令，需提供口令".into()))?;
        sk.decrypt(pw).map_err(|_| AppError::BadPassword)?
    } else {
        sk
    };
    let text = decrypted
        .to_openssh(LineEnding::LF)
        .map_err(|_| AppError::Crypto)?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssh_key::LineEnding;

    fn gen_ed25519() -> PrivateKey {
        PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap()
    }

    #[test]
    fn parse_private_and_public_agree() {
        let sk = gen_ed25519();
        let priv_txt = sk.to_openssh(LineEnding::LF).unwrap();
        let pub_txt = sk.public_key().to_openssh().unwrap();

        let pi = parse_private_openssh(&priv_txt).unwrap();
        let pu = parse_public_openssh(&pub_txt).unwrap();
        assert_eq!(pi.algorithm, "ed25519");
        assert_eq!(pi.bits, Some(256));
        assert_eq!(pi.encrypted, Some(false));
        assert_eq!(pi.fingerprint, pu.fingerprint);
        assert!(pi.fingerprint.starts_with("SHA256:"));
    }

    #[test]
    fn encrypted_private_reports_encrypted_and_public_still_readable() {
        let sk = gen_ed25519();
        let enc = sk.encrypt(&mut rand_core::OsRng, "testpass").unwrap();
        let priv_txt = enc.to_openssh(LineEnding::LF).unwrap();

        let info = parse_private_openssh(&priv_txt).unwrap();
        assert_eq!(info.encrypted, Some(true));
        // 加密私钥仍能取出正确公钥指纹（与原始一致）。
        assert_eq!(
            info.fingerprint,
            sk.public_key().fingerprint(HashAlg::Sha256).to_string()
        );
    }

    #[test]
    fn passphrase_verification() {
        let sk = gen_ed25519();
        let enc = sk.encrypt(&mut rand_core::OsRng, "correct").unwrap();
        let priv_txt = enc.to_openssh(LineEnding::LF).unwrap();
        assert!(verify_passphrase(&priv_txt, "correct").is_ok());
        assert_eq!(
            verify_passphrase(&priv_txt, "wrong").unwrap_err().code(),
            "BAD_PASSWORD"
        );
    }

    #[test]
    fn pairing_detects_match_and_mismatch() {
        let a = gen_ed25519();
        let b = gen_ed25519();
        let a_priv = a.to_openssh(LineEnding::LF).unwrap();
        let a_pub = a.public_key().to_openssh().unwrap();
        let b_pub = b.public_key().to_openssh().unwrap();
        assert!(pair_matches(&a_priv, &a_pub).unwrap(), "同一对应匹配");
        assert!(!pair_matches(&a_priv, &b_pub).unwrap(), "张冠李戴应不匹配");
    }

    #[test]
    fn decrypt_yields_unencrypted_key_matching_public() {
        let sk = gen_ed25519();
        let orig_pub = sk.public_key().to_openssh().unwrap();
        let enc = sk.encrypt(&mut rand_core::OsRng, "pw123").unwrap();
        let enc_txt = enc.to_openssh(LineEnding::LF).unwrap();

        // 正确口令 → 解出不带口令的私钥，公钥不变。
        let plain = decrypt_to_openssh(&enc_txt, Some("pw123")).unwrap();
        let info = parse_private_openssh(&plain).unwrap();
        assert_eq!(info.encrypted, Some(false), "解密后应为不带口令");
        assert!(pair_matches(&plain, &orig_pub).unwrap());

        // 错误口令报错。
        assert_eq!(
            decrypt_to_openssh(&enc_txt, Some("wrong")).unwrap_err().code(),
            "BAD_PASSWORD"
        );
        // 缺口令报错。
        assert!(decrypt_to_openssh(&enc_txt, None).is_err());
    }

    #[test]
    fn decrypt_passthrough_for_unencrypted() {
        let sk = gen_ed25519();
        let txt = sk.to_openssh(LineEnding::LF).unwrap();
        let plain = decrypt_to_openssh(&txt, None).unwrap();
        assert_eq!(parse_private_openssh(&plain).unwrap().encrypted, Some(false));
    }

    #[test]
    fn format_detection() {
        let sk = gen_ed25519();
        let priv_txt = sk.to_openssh(LineEnding::LF).unwrap();
        assert_eq!(detect_format(&priv_txt), KeyFormat::OpenSsh);
        assert_eq!(
            detect_format("PuTTY-User-Key-File-2: ssh-rsa\n..."),
            KeyFormat::Ppk
        );
        assert_eq!(
            detect_format("-----BEGIN RSA PRIVATE KEY-----\nMII...\n-----END RSA PRIVATE KEY-----"),
            KeyFormat::Pem
        );
        assert_eq!(detect_format("ssh-ed25519 AAAA... user@host"), KeyFormat::OpenSsh);
    }
}
