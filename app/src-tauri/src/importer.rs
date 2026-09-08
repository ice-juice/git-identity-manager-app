//! 外部密钥导入入库：校验格式/配对/口令，加密写入 vault，并登记元数据。
//! 全程只把私钥密文写进 vault，绝不改动 `~/.ssh`。

use crate::error::{AppError, Result};
use crate::model::KeyRecord;
use crate::ssh::key::{self, KeyFormat};
use crate::store;
use crate::vault::Vault;

pub struct ImportRequest {
    pub private_text: String,
    /// 可选：显式提供的公钥（提供则做配对校验）。
    pub public_text: Option<String>,
    /// 带口令密钥的口令（校验并存入 vault）。
    pub passphrase: Option<String>,
    pub source_path: Option<String>,
    /// 可选备注名，缺省从公钥注释或算法推断。
    pub name: Option<String>,
}

fn now_iso8601() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn default_name(algo: &str, comment: &str) -> String {
    let c = comment.trim();
    if !c.is_empty() {
        // 取 @ 前的用户名或整体，做温和清洗。
        let base = c.split('@').next().unwrap_or(c);
        let cleaned: String = base
            .chars()
            .map(|ch| if ch.is_alphanumeric() { ch } else { '_' })
            .collect();
        if !cleaned.is_empty() {
            return format!("id_{}_{}", algo, cleaned);
        }
    }
    format!("id_{}", algo)
}

/// 执行导入，成功返回登记的密钥记录。
pub fn import_private_key(vault: &Vault, req: ImportRequest) -> Result<KeyRecord> {
    if !vault.is_unlocked() {
        return Err(AppError::Locked);
    }

    // 1. 格式检测：PPK / 传统 PEM 给出转换提示。
    match key::detect_format(&req.private_text) {
        KeyFormat::Ppk => {
            return Err(AppError::Invalid(
                "检测到 PuTTY .ppk 格式，OpenSSH 不识别。请先用 `puttygen 私钥.ppk -O private-openssh -o 新私钥` 转换后再导入。".into(),
            ))
        }
        KeyFormat::Pem => {
            // 传统 PEM 交给 ssh-key 尝试解析，失败再提示转换。
        }
        KeyFormat::Unknown => {
            return Err(AppError::Invalid("无法识别的密钥格式".into()));
        }
        KeyFormat::OpenSsh => {}
    }

    // 2. 解析私钥（即便带口令也能取指纹/公钥）。
    let info = key::parse_private_openssh(&req.private_text).map_err(|_| {
        AppError::Invalid(
            "私钥解析失败。若为传统 PEM，请用 `ssh-keygen -p -m RFC4716`/`-o` 转成 OpenSSH 新格式后再导入。".into(),
        )
    })?;
    let encrypted = info.encrypted.unwrap_or(false);

    // 3. 配对校验（若提供了显式公钥）。
    if let Some(pubtext) = &req.public_text {
        if !key::pair_matches(&req.private_text, pubtext)? {
            return Err(AppError::Invalid("提供的公钥与私钥不配对，疑似张冠李戴".into()));
        }
    }

    // 4. 口令处理：带口令必须提供且正确。
    if encrypted {
        match &req.passphrase {
            Some(pw) => key::verify_passphrase(&req.private_text, pw)?,
            None => {
                return Err(AppError::Invalid("该私钥带口令，请提供口令以校验并入库".into()))
            }
        }
    }

    // 5. 查重（按指纹）。
    let mut data = store::load_data(vault)?;
    if data.keys.iter().any(|k| k.fingerprint == info.fingerprint) {
        return Err(AppError::Invalid(format!(
            "已存在相同指纹的密钥：{}",
            info.fingerprint
        )));
    }

    // 6. 弱密钥判定（RSA < 2048）。
    let weak = info.algorithm == "rsa" && info.bits.map(|b| b < 2048).unwrap_or(false);

    // 7. 加密写入私钥文件 + 登记元数据 + 存口令。
    let key_id = uuid::Uuid::new_v4().to_string();
    store::save_key(vault, &key_id, req.private_text.as_bytes())?;

    if encrypted {
        if let Some(pw) = &req.passphrase {
            let mut secrets = store::load_secrets(vault)?;
            secrets.key_passphrases.insert(key_id.clone(), pw.clone());
            store::save_secrets(vault, &secrets)?;
        }
    }

    let record = KeyRecord {
        id: key_id,
        name: req
            .name
            .clone()
            .unwrap_or_else(|| default_name(&info.algorithm, &info.comment)),
        algorithm: info.algorithm,
        fingerprint: info.fingerprint,
        public_openssh: info.public_openssh,
        bits: info.bits,
        has_passphrase: encrypted,
        weak,
        source_path: req.source_path.clone(),
        imported_at: now_iso8601(),
    };
    data.keys.push(record.clone());
    store::save_data(vault, &data)?;

    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::header::KdfParams;
    use crate::vault::kdf::{ITERS_FLOOR, MEM_FLOOR_KIB};
    use ssh_key::{Algorithm, LineEnding, PrivateKey};
    use std::path::PathBuf;

    fn unlocked_vault() -> (Vault, PathBuf) {
        let root = std::env::temp_dir().join(format!("gam-imp-{}", uuid::Uuid::new_v4()));
        let kdf = KdfParams::new(MEM_FLOOR_KIB, ITERS_FLOOR, 1);
        let (v, _rec) = Vault::init(&root, "pw", kdf).unwrap();
        (v, root)
    }

    fn gen() -> PrivateKey {
        PrivateKey::random(&mut rand_core::OsRng, Algorithm::Ed25519).unwrap()
    }

    #[test]
    fn import_openssh_key_stores_encrypted_and_registers() {
        let (v, root) = unlocked_vault();
        let sk = gen();
        let priv_txt = sk.to_openssh(LineEnding::LF).unwrap().to_string();
        let rec = import_private_key(
            &v,
            ImportRequest {
                private_text: priv_txt,
                public_text: None,
                passphrase: None,
                source_path: Some(r"D:\old\id_ed25519".into()),
                name: None,
            },
        )
        .unwrap();

        assert_eq!(rec.algorithm, "ed25519");
        assert!(!rec.has_passphrase);
        // 已登记且能通过二次验证读回公钥（元数据）。
        let data = store::load_data(&v).unwrap();
        assert_eq!(data.keys.len(), 1);
        assert_eq!(data.keys[0].fingerprint, rec.fingerprint);
        // 私钥密文文件存在且可解回。
        let back = store::load_key(&v, &rec.id).unwrap();
        assert!(String::from_utf8_lossy(&back).contains("OPENSSH PRIVATE KEY"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn import_encrypted_requires_and_stores_passphrase() {
        let (v, root) = unlocked_vault();
        let enc = gen().encrypt(&mut rand_core::OsRng, "topsecret").unwrap();
        let priv_txt = enc.to_openssh(LineEnding::LF).unwrap().to_string();

        // 不给口令应失败。
        assert!(import_private_key(
            &v,
            ImportRequest {
                private_text: priv_txt.clone(),
                public_text: None,
                passphrase: None,
                source_path: None,
                name: None,
            }
        )
        .is_err());

        // 给正确口令成功，且口令入库（不落明文）。
        let rec = import_private_key(
            &v,
            ImportRequest {
                private_text: priv_txt,
                public_text: None,
                passphrase: Some("topsecret".into()),
                source_path: None,
                name: Some("id_ed25519_techn".into()),
            },
        )
        .unwrap();
        assert!(rec.has_passphrase);
        let secrets = store::load_secrets(&v).unwrap();
        assert_eq!(secrets.key_passphrases.get(&rec.id).unwrap(), "topsecret");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn duplicate_fingerprint_rejected() {
        let (v, root) = unlocked_vault();
        let sk = gen();
        let priv_txt = sk.to_openssh(LineEnding::LF).unwrap().to_string();
        let req = |t: String| ImportRequest {
            private_text: t,
            public_text: None,
            passphrase: None,
            source_path: None,
            name: None,
        };
        assert!(import_private_key(&v, req(priv_txt.clone())).is_ok());
        assert!(import_private_key(&v, req(priv_txt)).is_err(), "重复指纹应拒绝");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn mismatched_public_rejected() {
        let (v, root) = unlocked_vault();
        let a = gen();
        let b = gen();
        let a_priv = a.to_openssh(LineEnding::LF).unwrap().to_string();
        let b_pub = b.public_key().to_openssh().unwrap();
        let err = import_private_key(
            &v,
            ImportRequest {
                private_text: a_priv,
                public_text: Some(b_pub),
                passphrase: None,
                source_path: None,
                name: None,
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "INVALID");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ppk_gives_conversion_hint() {
        let (v, root) = unlocked_vault();
        let err = import_private_key(
            &v,
            ImportRequest {
                private_text: "PuTTY-User-Key-File-2: ssh-rsa\nEncryption: none\n".into(),
                public_text: None,
                passphrase: None,
                source_path: None,
                name: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("puttygen"));
        std::fs::remove_dir_all(&root).ok();
    }
}
