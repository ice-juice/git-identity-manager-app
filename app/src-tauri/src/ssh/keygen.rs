//! 密钥生成：ed25519（默认），强制随机强口令（落地 M0 默认方案）。
//! 生成的私钥文件本身即密文（OpenSSH bcrypt-pbkdf + AES）。

use crate::error::{AppError, Result};
use crate::vault::crypto::random_vec;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine};
use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey};
use zeroize::Zeroize;

/// 生成结果（私钥为带口令的密文；口令与私钥都要存进 vault）。
pub struct GeneratedKey {
    /// 带口令加密的 OpenSSH 私钥文本。
    pub private_openssh_encrypted: String,
    pub public_openssh: String,
    pub fingerprint: String,
    /// 随机口令（调用方负责写入 vault 后清零）。
    pub passphrase: String,
}

/// 生成一个 24 字节熵的强随机口令（URL-safe base64，约 192bit）。
pub fn random_passphrase() -> String {
    let mut bytes = random_vec(24);
    let s = B64URL.encode(&bytes);
    bytes.zeroize();
    s
}

/// 生成 ed25519 密钥对，带随机强口令。
pub fn generate_ed25519(comment: &str) -> Result<GeneratedKey> {
    let mut sk = PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519)
        .map_err(|e| AppError::Other(format!("密钥生成失败：{e}")))?;
    sk.set_comment(comment);

    let public_openssh = sk
        .public_key()
        .to_openssh()
        .map_err(|_| AppError::Crypto)?;
    let fingerprint = sk.public_key().fingerprint(HashAlg::Sha256).to_string();

    let passphrase = random_passphrase();
    let encrypted = sk
        .encrypt(&mut rand_core::OsRng, &passphrase)
        .map_err(|_| AppError::Crypto)?;
    let private_openssh_encrypted = encrypted
        .to_openssh(LineEnding::LF)
        .map_err(|_| AppError::Crypto)?
        .to_string();

    Ok(GeneratedKey {
        private_openssh_encrypted,
        public_openssh,
        fingerprint,
        passphrase,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::key;

    #[test]
    fn generated_key_is_encrypted_and_pub_matches() {
        let g = generate_ed25519("techn4950@gam").unwrap();
        // 私钥带口令。
        let info = key::parse_private_openssh(&g.private_openssh_encrypted).unwrap();
        assert_eq!(info.encrypted, Some(true));
        assert_eq!(info.algorithm, "ed25519");
        // 公钥与私钥配对、指纹一致。
        assert!(key::pair_matches(&g.private_openssh_encrypted, &g.public_openssh).unwrap());
        assert_eq!(info.fingerprint, g.fingerprint);
    }

    #[test]
    fn generated_passphrase_unlocks_private_key() {
        let g = generate_ed25519("x").unwrap();
        assert!(key::verify_passphrase(&g.private_openssh_encrypted, &g.passphrase).is_ok());
        assert!(key::verify_passphrase(&g.private_openssh_encrypted, "wrong").is_err());
    }

    #[test]
    fn passphrases_are_random_and_strong() {
        let a = random_passphrase();
        let b = random_passphrase();
        assert_ne!(a, b);
        assert!(a.len() >= 30, "约 192bit 熵，base64 长度足够");
    }
}
