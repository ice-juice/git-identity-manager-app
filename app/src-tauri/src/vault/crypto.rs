//! 对称加密（XChaCha20-Poly1305）、子密钥派生（HKDF-SHA256）、随机数。
//!
//! 所有机密材料用 `zeroize` 管理；子密钥派生标签在此集中定义，
//! 包含云同步用标签（v1.1 才使用），以保证工作空间格式向前兼容。

use crate::error::{AppError, Result};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const KEY_LEN: usize = 32;
pub const XNONCE_LEN: usize = 24;
pub const SALT_LEN: usize = 16;

// ---- 子密钥用途标签（一经发布不可更改）----
/// 私钥文件加密
pub const LABEL_KEYFILE: &[u8] = b"gam-key-file-v1";
/// 身份/设置/机密元数据加密
pub const LABEL_METADATA: &[u8] = b"gam-metadata-v1";
/// 云对象内容加密（v1.1）
pub const LABEL_SYNC_OBJECT: &[u8] = b"gam-sync-object-v1";
/// 云对象名 HMAC（v1.1）
pub const LABEL_SYNC_NAME: &[u8] = b"gam-sync-name-v1";

/// 32 字节主密钥，Drop 时自动清零。
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MasterKey(pub [u8; KEY_LEN]);

// 手写 Debug：绝不打印密钥字节。
impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MasterKey(***redacted***)")
    }
}

impl MasterKey {
    pub fn random() -> Self {
        let mut k = [0u8; KEY_LEN];
        fill_random(&mut k);
        MasterKey(k)
    }
    pub fn from_bytes(b: [u8; KEY_LEN]) -> Self {
        MasterKey(b)
    }
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    /// 从主密钥派生 32 字节子密钥（HKDF-SHA256，info=label）。
    pub fn subkey(&self, label: &[u8]) -> [u8; KEY_LEN] {
        let hk = Hkdf::<Sha256>::new(None, &self.0);
        let mut okm = [0u8; KEY_LEN];
        hk.expand(label, &mut okm)
            .expect("32 字节输出长度对 HKDF-SHA256 始终有效");
        okm
    }

    /// 云对象名 HMAC（十六进制），用于隐藏逻辑路径信息（v1.1）。
    pub fn object_name(&self, logical_path: &str) -> String {
        let key = self.subkey(LABEL_SYNC_NAME);
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&key).expect("HMAC 接受任意长度密钥");
        mac.update(logical_path.as_bytes());
        let out = mac.finalize().into_bytes();
        hex_encode(&out)
    }
}

/// 用 CSPRNG 填充随机字节。
pub fn fill_random(buf: &mut [u8]) {
    getrandom::getrandom(buf).expect("操作系统 CSPRNG 不可用");
}

pub fn random_vec(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    fill_random(&mut v);
    v
}

/// XChaCha20-Poly1305 加密，返回 密文||tag。
pub fn aead_encrypt(key: &[u8; KEY_LEN], nonce: &[u8; XNONCE_LEN], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .encrypt(XNonce::from_slice(nonce), plaintext)
        .map_err(|_| AppError::Crypto)
}

/// XChaCha20-Poly1305 解密（AEAD，tag 校验失败即返回错误）。
pub fn aead_decrypt(key: &[u8; KEY_LEN], nonce: &[u8; XNONCE_LEN], ciphertext: &[u8]) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| AppError::Crypto)
}

pub fn new_nonce() -> [u8; XNONCE_LEN] {
    let mut n = [0u8; XNONCE_LEN];
    fill_random(&mut n);
    n
}

pub fn new_salt() -> [u8; SALT_LEN] {
    let mut s = [0u8; SALT_LEN];
    fill_random(&mut s);
    s
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aead_roundtrip() {
        let key = [7u8; KEY_LEN];
        let nonce = new_nonce();
        let msg = b"hello vault";
        let ct = aead_encrypt(&key, &nonce, msg).unwrap();
        assert_ne!(&ct[..], &msg[..]);
        let pt = aead_decrypt(&key, &nonce, &ct).unwrap();
        assert_eq!(pt, msg);
    }

    #[test]
    fn aead_tamper_is_rejected() {
        let key = [3u8; KEY_LEN];
        let nonce = new_nonce();
        let mut ct = aead_encrypt(&key, &nonce, b"secret").unwrap();
        let last = ct.len() - 1;
        ct[last] ^= 0x01; // 篡改 tag
        assert!(aead_decrypt(&key, &nonce, &ct).is_err());
    }

    #[test]
    fn aead_wrong_key_rejected() {
        let nonce = new_nonce();
        let ct = aead_encrypt(&[1u8; KEY_LEN], &nonce, b"data").unwrap();
        assert!(aead_decrypt(&[2u8; KEY_LEN], &nonce, &ct).is_err());
    }

    #[test]
    fn subkey_is_deterministic_and_label_separated() {
        let mk = MasterKey([9u8; KEY_LEN]);
        let a1 = mk.subkey(LABEL_KEYFILE);
        let a2 = mk.subkey(LABEL_KEYFILE);
        let b = mk.subkey(LABEL_METADATA);
        assert_eq!(a1, a2, "同标签派生应确定");
        assert_ne!(a1, b, "不同标签派生应不同");
    }

    #[test]
    fn object_name_hides_path_and_is_stable() {
        let mk = MasterKey([5u8; KEY_LEN]);
        let n1 = mk.object_name("keys/id_ed25519_techn.enc");
        let n2 = mk.object_name("keys/id_ed25519_techn.enc");
        let n3 = mk.object_name("keys/id_ed25519_juice.enc");
        assert_eq!(n1, n2);
        assert_ne!(n1, n3);
        assert_eq!(n1.len(), 64); // sha256 hex
        assert!(!n1.contains("techn"));
    }

    #[test]
    fn random_is_nonzero_and_unique() {
        let a = random_vec(32);
        let b = random_vec(32);
        assert_ne!(a, b);
        assert_ne!(a, vec![0u8; 32]);
    }
}
