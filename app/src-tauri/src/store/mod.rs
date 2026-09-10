//! 加密数据持久化：用 vault 派生的子密钥加密 identities/secrets/key 文件。
//! 文件格式：`nonce(24) || XChaCha20-Poly1305 密文`。

use crate::error::{AppError, Result};
use crate::model::{AccountData, Secrets, TotpData, VaultData};
use crate::vault::atomic_write;
use crate::vault::crypto::{self, KEY_LEN, LABEL_KEYFILE, LABEL_METADATA, XNONCE_LEN};
use crate::vault::Vault;
use std::path::PathBuf;

fn seal(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>> {
    let nonce = crypto::new_nonce();
    let ct = crypto::aead_encrypt(key, &nonce, plaintext)?;
    let mut out = Vec::with_capacity(XNONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn open(key: &[u8; KEY_LEN], data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < XNONCE_LEN {
        return Err(AppError::Crypto);
    }
    let (n, ct) = data.split_at(XNONCE_LEN);
    let mut nonce = [0u8; XNONCE_LEN];
    nonce.copy_from_slice(n);
    crypto::aead_decrypt(key, &nonce, ct)
}

fn data_path(vault: &Vault) -> PathBuf {
    vault.root().join("data").join("identities.enc")
}
fn secrets_path(vault: &Vault) -> PathBuf {
    vault.root().join("data").join("secrets.enc")
}
fn key_path(vault: &Vault, key_id: &str) -> PathBuf {
    vault.root().join("keys").join(format!("{key_id}.enc"))
}
fn totp_path(vault: &Vault) -> PathBuf {
    vault.root().join("data").join("totp.enc")
}
fn accounts_path(vault: &Vault) -> PathBuf {
    vault.root().join("data").join("accounts.enc")
}
fn icon_path(vault: &Vault, hash: &str) -> PathBuf {
    vault.root().join("icons").join(format!("{hash}.webp"))
}

/// 读取元数据容器（不存在时返回默认）。需已解锁。
pub fn load_data(vault: &Vault) -> Result<VaultData> {
    let key = vault.subkey(LABEL_METADATA)?;
    let path = data_path(vault);
    if !path.exists() {
        return Ok(VaultData::default());
    }
    let raw = std::fs::read(&path)?;
    let plain = open(&key, &raw)?;
    Ok(serde_json::from_slice(&plain)?)
}

pub fn save_data(vault: &Vault, data: &VaultData) -> Result<()> {
    let key = vault.subkey(LABEL_METADATA)?;
    let json = serde_json::to_vec(data)?;
    let sealed = seal(&key, &json)?;
    atomic_write(&data_path(vault), &sealed)
}

pub fn load_secrets(vault: &Vault) -> Result<Secrets> {
    let key = vault.subkey(LABEL_METADATA)?;
    let path = secrets_path(vault);
    if !path.exists() {
        return Ok(Secrets::default());
    }
    let raw = std::fs::read(&path)?;
    let plain = open(&key, &raw)?;
    Ok(serde_json::from_slice(&plain)?)
}

pub fn save_secrets(vault: &Vault, secrets: &Secrets) -> Result<()> {
    let key = vault.subkey(LABEL_METADATA)?;
    let json = serde_json::to_vec(secrets)?;
    let sealed = seal(&key, &json)?;
    atomic_write(&secrets_path(vault), &sealed)
}

/// 保存私钥密文副本（keys/<id>.enc）。
pub fn save_key(vault: &Vault, key_id: &str, private_bytes: &[u8]) -> Result<()> {
    let key = vault.subkey(LABEL_KEYFILE)?;
    let sealed = seal(&key, private_bytes)?;
    atomic_write(&key_path(vault, key_id), &sealed)
}

/// 读取私钥密文副本明文（需已解锁）。
pub fn load_key(vault: &Vault, key_id: &str) -> Result<Vec<u8>> {
    let key = vault.subkey(LABEL_KEYFILE)?;
    let raw = std::fs::read(key_path(vault, key_id))?;
    open(&key, &raw)
}

pub fn key_exists(vault: &Vault, key_id: &str) -> bool {
    key_path(vault, key_id).exists()
}

pub fn load_totp(vault: &Vault) -> Result<TotpData> {
    let key = vault.subkey(LABEL_METADATA)?;
    let path = totp_path(vault);
    if !path.exists() {
        return Ok(TotpData::default());
    }
    let raw = std::fs::read(&path)?;
    let plain = open(&key, &raw)?;
    Ok(serde_json::from_slice(&plain)?)
}

pub fn save_totp(vault: &Vault, data: &TotpData) -> Result<()> {
    let key = vault.subkey(LABEL_METADATA)?;
    let json = serde_json::to_vec(data)?;
    let sealed = seal(&key, &json)?;
    atomic_write(&totp_path(vault), &sealed)
}

pub fn load_accounts(vault: &Vault) -> Result<AccountData> {
    let key = vault.subkey(LABEL_METADATA)?;
    let path = accounts_path(vault);
    if !path.exists() {
        return Ok(AccountData::default());
    }
    let raw = std::fs::read(&path)?;
    let plain = open(&key, &raw)?;
    Ok(serde_json::from_slice(&plain)?)
}

pub fn save_accounts(vault: &Vault, data: &AccountData) -> Result<()> {
    let key = vault.subkey(LABEL_METADATA)?;
    let json = serde_json::to_vec(data)?;
    let sealed = seal(&key, &json)?;
    atomic_write(&accounts_path(vault), &sealed)
}

pub fn save_icon(vault: &Vault, hash: &str, webp: &[u8]) -> Result<()> {
    atomic_write(&icon_path(vault, hash), webp)
}

pub fn load_icon(vault: &Vault, hash: &str) -> Result<Vec<u8>> {
    let path = icon_path(vault, hash);
    if !path.is_file() {
        return Err(AppError::Invalid("自定义图标不存在".into()));
    }
    Ok(std::fs::read(path)?)
}

pub fn icon_exists(vault: &Vault, hash: &str) -> bool {
    icon_path(vault, hash).is_file()
}

pub fn list_icon_hashes(vault: &Vault) -> Vec<String> {
    let dir = vault.root().join("icons");
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name();
        let Some(s) = name.to_str() else { continue };
        if let Some(hash) = s.strip_suffix(".webp") {
            if !hash.is_empty() {
                out.push(hash.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Identity, KeyRecord};
    use crate::vault::header::KdfParams;
    use crate::vault::kdf::{ITERS_FLOOR, MEM_FLOOR_KIB};

    fn unlocked_vault() -> (Vault, PathBuf) {
        let root = std::env::temp_dir().join(format!("gam-store-{}", uuid::Uuid::new_v4()));
        let kdf = KdfParams::new(MEM_FLOOR_KIB, ITERS_FLOOR, 1);
        let (v, _rec) = Vault::init(&root, "pw", kdf).unwrap();
        assert!(v.is_unlocked());
        // 确保 subkey 可用。
        let _ = v.subkey(LABEL_METADATA).unwrap();
        (v, root)
    }

    #[test]
    fn data_roundtrip_encrypted_on_disk() {
        let (v, root) = unlocked_vault();
        let mut data = VaultData::default();
        data.keys.push(KeyRecord {
            id: "k1".into(),
            name: "id_ed25519_techn".into(),
            algorithm: "ed25519".into(),
            fingerprint: "SHA256:abc".into(),
            public_openssh: "ssh-ed25519 AAAA techn".into(),
            bits: Some(256),
            has_passphrase: true,
            weak: false,
            source_path: None,
            deployed_path: None,
            imported_at: "now".into(),
        });
        data.identities.push(Identity {
            id: "i1".into(),
            name: "techn4950".into(),
            platform: "github".into(),
            host_alias: "github-techn".into(),
            real_host: "github.com".into(),
            user: "git".into(),
            email: None,
            git_user_name: None,
            key_id: Some("k1".into()),
            owners: vec!["vortaq-trad".into()],
            strict_mode: false,
            updated_at: String::new(),
        });
        save_data(&v, &data).unwrap();

        // 磁盘上不得出现明文标识。
        let raw = std::fs::read(data_path(&v)).unwrap();
        let as_text = String::from_utf8_lossy(&raw);
        assert!(!as_text.contains("techn4950"));
        assert!(!as_text.contains("vortaq-trad"));

        let loaded = load_data(&v).unwrap();
        assert_eq!(loaded.identities, data.identities);
        assert_eq!(loaded.keys, data.keys);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn key_file_roundtrip() {
        let (v, root) = unlocked_vault();
        let secret = b"-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END-----\n";
        save_key(&v, "k1", secret).unwrap();
        assert!(key_exists(&v, "k1"));
        let raw = std::fs::read(key_path(&v, "k1")).unwrap();
        assert!(!raw.windows(4).any(|w| w == b"fake"), "密文不含明文片段");
        let got = load_key(&v, "k1").unwrap();
        assert_eq!(got, secret);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn secrets_roundtrip() {
        let (v, root) = unlocked_vault();
        let mut s = Secrets::default();
        s.key_passphrases.insert("k1".into(), "s3cr3t-pass".into());
        s.github_pat = Some("ghp_xxx".into());
        save_secrets(&v, &s).unwrap();
        let raw = std::fs::read(secrets_path(&v)).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("s3cr3t-pass"));
        let got = load_secrets(&v).unwrap();
        assert_eq!(got.key_passphrases.get("k1").unwrap(), "s3cr3t-pass");
        assert_eq!(got.github_pat.as_deref(), Some("ghp_xxx"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn totp_roundtrip_encrypted() {
        let (v, root) = unlocked_vault();
        let mut data = TotpData::default();
        data.entries.push(crate::model::TotpEntry {
            id: "t1".into(),
            issuer: "GitHub".into(),
            account: "techn4950".into(),
            note: Some("secret-note".into()),
            url: None,
            group: None,
            algorithm: "SHA1".into(),
            digits: 6,
            period: 30,
            icon: None,
            sort_order: 0,
            created_at: "now".into(),
            updated_at: "now".into(),
        });
        save_totp(&v, &data).unwrap();
        let raw = std::fs::read(totp_path(&v)).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("techn4950"));
        let loaded = load_totp(&v).unwrap();
        assert_eq!(loaded.entries, data.entries);
        std::fs::remove_dir_all(&root).ok();
    }
}
