//! 信封加密：用访问密码（Argon2id）与恢复密钥（HKDF）各自包裹同一把主密钥 MK。
//!
//! 由此获得：改密码毫秒级（仅重包裹）、恢复密钥可轮换、两者互不依赖。

use crate::error::{AppError, Result};
use crate::vault::crypto::{
    aead_decrypt, aead_encrypt, new_nonce, new_salt, MasterKey, KEY_LEN, SALT_LEN, XNONCE_LEN,
};
use crate::vault::header::{Envelope, KdfParams};
use crate::vault::kdf::derive_kek;
use crate::vault::recovery::RecoverySecret;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hkdf::Hkdf;
use sha2::Sha256;

const RECOVERY_HKDF_INFO: &[u8] = b"gam-recovery-kek-v1";
const BIOMETRIC_HKDF_INFO: &[u8] = b"gam-biometric-kek-v1";

fn b64(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

fn unb64(s: &str) -> Result<Vec<u8>> {
    B64.decode(s).map_err(|_| AppError::Serde("base64 解码失败".into()))
}

fn to_nonce(v: &[u8]) -> Result<[u8; XNONCE_LEN]> {
    if v.len() != XNONCE_LEN {
        return Err(AppError::Crypto);
    }
    let mut n = [0u8; XNONCE_LEN];
    n.copy_from_slice(v);
    Ok(n)
}

/// 恢复密钥经 HKDF-SHA256 派生 KEK（其本身已是高熵，无需慢哈希）。
fn recovery_kek(secret: &RecoverySecret, salt: &[u8]) -> [u8; KEY_LEN] {
    let hk = Hkdf::<Sha256>::new(Some(salt), secret.as_bytes());
    let mut okm = [0u8; KEY_LEN];
    hk.expand(RECOVERY_HKDF_INFO, &mut okm)
        .expect("32 字节输出有效");
    okm
}

/// 用访问密码包裹 MK。
pub fn wrap_with_password(mk: &MasterKey, password: &str, kdf: &KdfParams) -> Result<Envelope> {
    let salt = new_salt();
    let kek = derive_kek(password.as_bytes(), &salt, kdf)?;
    let nonce = new_nonce();
    let wrapped = aead_encrypt(&kek, &nonce, mk.as_bytes())?;
    Ok(Envelope {
        salt: b64(&salt),
        nonce: b64(&nonce),
        wrapped_key: b64(&wrapped),
    })
}

/// 用访问密码解开 MK（失败即密码错误或数据损坏）。
pub fn unwrap_with_password(env: &Envelope, password: &str, kdf: &KdfParams) -> Result<MasterKey> {
    let salt = unb64(&env.salt)?;
    if salt.len() != SALT_LEN {
        return Err(AppError::Crypto);
    }
    let kek = derive_kek(password.as_bytes(), &salt, kdf)?;
    let nonce = to_nonce(&unb64(&env.nonce)?)?;
    let wrapped = unb64(&env.wrapped_key)?;
    let mk_bytes = aead_decrypt(&kek, &nonce, &wrapped).map_err(|_| AppError::BadPassword)?;
    if mk_bytes.len() != KEY_LEN {
        return Err(AppError::Crypto);
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(&mk_bytes);
    Ok(MasterKey::from_bytes(arr))
}

/// 用恢复密钥包裹 MK。
pub fn wrap_with_recovery(mk: &MasterKey, secret: &RecoverySecret) -> Result<Envelope> {
    let salt = new_salt();
    let kek = recovery_kek(secret, &salt);
    let nonce = new_nonce();
    let wrapped = aead_encrypt(&kek, &nonce, mk.as_bytes())?;
    Ok(Envelope {
        salt: b64(&salt),
        nonce: b64(&nonce),
        wrapped_key: b64(&wrapped),
    })
}

/// 用恢复密钥解开 MK。
pub fn unwrap_with_recovery(env: &Envelope, secret: &RecoverySecret) -> Result<MasterKey> {
    let salt = unb64(&env.salt)?;
    let kek = recovery_kek(secret, &salt);
    let nonce = to_nonce(&unb64(&env.nonce)?)?;
    let wrapped = unb64(&env.wrapped_key)?;
    let mk_bytes = aead_decrypt(&kek, &nonce, &wrapped)
        .map_err(|_| AppError::BadRecoveryKey("无法用该恢复密钥解开数据".into()))?;
    if mk_bytes.len() != KEY_LEN {
        return Err(AppError::Crypto);
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(&mk_bytes);
    Ok(MasterKey::from_bytes(arr))
}

fn biometric_kek(raw: &[u8], salt: &[u8]) -> [u8; KEY_LEN] {
    let hk = Hkdf::<Sha256>::new(Some(salt), raw);
    let mut okm = [0u8; KEY_LEN];
    hk.expand(BIOMETRIC_HKDF_INFO, &mut okm)
        .expect("32 字节输出有效");
    okm
}

/// 用生物识别硬件输出包裹 MK（本机第三信封，不上云）。
pub fn wrap_with_biometric(mk: &MasterKey, raw: &[u8]) -> Result<Envelope> {
    if raw.is_empty() {
        return Err(AppError::Crypto);
    }
    let salt = new_salt();
    let kek = biometric_kek(raw, &salt);
    let nonce = new_nonce();
    let wrapped = aead_encrypt(&kek, &nonce, mk.as_bytes())?;
    Ok(Envelope {
        salt: b64(&salt),
        nonce: b64(&nonce),
        wrapped_key: b64(&wrapped),
    })
}

/// 用生物识别硬件输出解开 MK。
pub fn unwrap_with_biometric(env: &Envelope, raw: &[u8]) -> Result<MasterKey> {
    let salt = unb64(&env.salt)?;
    if salt.len() != SALT_LEN {
        return Err(AppError::Crypto);
    }
    let kek = biometric_kek(raw, &salt);
    let nonce = to_nonce(&unb64(&env.nonce)?)?;
    let wrapped = unb64(&env.wrapped_key)?;
    let mk_bytes = aead_decrypt(&kek, &nonce, &wrapped).map_err(|_| AppError::BiometricStale)?;
    if mk_bytes.len() != KEY_LEN {
        return Err(AppError::Crypto);
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(&mk_bytes);
    Ok(MasterKey::from_bytes(arr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::kdf::{ITERS_FLOOR, MEM_FLOOR_KIB};

    fn fast_kdf() -> KdfParams {
        KdfParams::new(MEM_FLOOR_KIB, ITERS_FLOOR, 1)
    }

    #[test]
    fn password_envelope_roundtrip() {
        let mk = MasterKey([42u8; KEY_LEN]);
        let kdf = fast_kdf();
        let env = wrap_with_password(&mk, "correct horse", &kdf).unwrap();
        let got = unwrap_with_password(&env, "correct horse", &kdf).unwrap();
        assert_eq!(got.as_bytes(), mk.as_bytes());
    }

    #[test]
    fn wrong_password_rejected() {
        let mk = MasterKey([1u8; KEY_LEN]);
        let kdf = fast_kdf();
        let env = wrap_with_password(&mk, "right", &kdf).unwrap();
        let err = unwrap_with_password(&env, "wrong", &kdf).unwrap_err();
        assert_eq!(err.code(), "BAD_PASSWORD");
    }

    #[test]
    fn recovery_envelope_roundtrip() {
        let mk = MasterKey([7u8; KEY_LEN]);
        let secret = RecoverySecret([9u8; KEY_LEN]);
        let env = wrap_with_recovery(&mk, &secret).unwrap();
        let got = unwrap_with_recovery(&env, &secret).unwrap();
        assert_eq!(got.as_bytes(), mk.as_bytes());
    }

    #[test]
    fn wrong_recovery_rejected() {
        let mk = MasterKey([7u8; KEY_LEN]);
        let env = wrap_with_recovery(&mk, &RecoverySecret([9u8; KEY_LEN])).unwrap();
        assert!(unwrap_with_recovery(&env, &RecoverySecret([8u8; KEY_LEN])).is_err());
    }

    #[test]
    fn both_envelopes_yield_same_mk() {
        // 模拟真实：同一把 MK 被两种信封包裹，均可解回。
        let mk = MasterKey([123u8; KEY_LEN]);
        let kdf = fast_kdf();
        let secret = RecoverySecret([200u8; KEY_LEN]);
        let pe = wrap_with_password(&mk, "pw", &kdf).unwrap();
        let re = wrap_with_recovery(&mk, &secret).unwrap();
        let from_pw = unwrap_with_password(&pe, "pw", &kdf).unwrap();
        let from_rec = unwrap_with_recovery(&re, &secret).unwrap();
        assert_eq!(from_pw.as_bytes(), from_rec.as_bytes());
        assert_eq!(from_pw.as_bytes(), mk.as_bytes());
    }

    #[test]
    fn biometric_envelope_roundtrip() {
        let mk = MasterKey([11u8; KEY_LEN]);
        let raw = [9u8; 64];
        let env = wrap_with_biometric(&mk, &raw).unwrap();
        let got = unwrap_with_biometric(&env, &raw).unwrap();
        assert_eq!(got.as_bytes(), mk.as_bytes());
        assert!(unwrap_with_biometric(&env, &[8u8; 64]).is_err());
    }
}
