//! 加密工作空间（vault）：所有密钥资产的加密权威副本。
//!
//! 目录结构（见 规划设计.md §4.7）：
//! ```text
//! <工作空间>/
//! ├── vault.json      # 头部（版本 / KDF 参数 / 两个信封），唯一不加密文件，无明文机密
//! ├── data/           # identities.enc / secrets.enc / settings.enc（M2+）
//! ├── keys/           # <keyId>.enc 私钥加密副本（M2+）
//! ├── backups/        # 本地历史快照
//! ├── sync/           # manifest.enc（v1.1）
//! └── audit.log       # 脱敏操作日志
//! ```

pub mod crypto;
pub mod envelope;
pub mod header;
pub mod kdf;
pub mod recovery;

use crate::error::{AppError, Result};
use crypto::MasterKey;
use header::{Envelopes, KdfParams, VaultHeader, VAULT_VERSION};
use std::path::{Path, PathBuf};

pub const VAULT_FILE: &str = "vault.json";

/// vault 管理器：持有工作空间路径、头部；解锁后在内存中持有 MK。
#[derive(Clone)]
pub struct Vault {
    root: PathBuf,
    header: VaultHeader,
    /// Some 表示已解锁；MasterKey 在 Drop 时清零。
    mk: Option<MasterKey>,
}

impl Vault {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn is_unlocked(&self) -> bool {
        self.mk.is_some()
    }
    pub fn workspace_id(&self) -> &str {
        &self.header.workspace_id
    }
    pub fn header(&self) -> &VaultHeader {
        &self.header
    }

    fn vault_path(root: &Path) -> PathBuf {
        root.join(VAULT_FILE)
    }

    /// 目标目录是否已是一个已初始化的工作空间。
    pub fn exists(root: &Path) -> bool {
        Self::vault_path(root).is_file()
    }

    /// 初始化新工作空间：生成 MK 与恢复密钥，写 vault.json 与子目录骨架。
    /// 返回 (Vault[已解锁], 恢复密钥展示串)。
    pub fn init(root: &Path, password: &str, kdf: KdfParams) -> Result<(Self, String)> {
        if password.is_empty() {
            return Err(AppError::Invalid("访问密码不能为空".into()));
        }
        if Self::exists(root) {
            return Err(AppError::AlreadyInitialized(root.display().to_string()));
        }
        std::fs::create_dir_all(root)?;
        for sub in ["data", "keys", "backups", "sync", "ssh-keys", "ssh", "icons"] {
            std::fs::create_dir_all(root.join(sub))?;
        }

        let mut kdf = kdf;
        kdf.clamp_to_safe_bounds();

        let mk = MasterKey::random();
        let (recovery_secret, recovery_display) = recovery::generate();

        let password_env = envelope::wrap_with_password(&mk, password, &kdf)?;
        let recovery_env = envelope::wrap_with_recovery(&mk, &recovery_secret)?;

        let header = VaultHeader {
            version: VAULT_VERSION,
            workspace_id: uuid::Uuid::new_v4().to_string(),
            created_at: now_iso8601(),
            kdf,
            envelopes: Envelopes {
                password: password_env,
                recovery: recovery_env,
            },
        };

        let vault = Vault {
            root: root.to_path_buf(),
            header,
            mk: Some(mk),
        };
        vault.persist_header()?;
        Ok((vault, recovery_display))
    }

    /// 加载已有工作空间的头部（处于锁定状态）。
    pub fn load(root: &Path) -> Result<Self> {
        let path = Self::vault_path(root);
        if !path.is_file() {
            return Err(AppError::NotInitialized);
        }
        let raw = std::fs::read_to_string(&path)?;
        let mut header: VaultHeader = serde_json::from_str(&raw)?;
        // 防头部被篡改成弱/非法 KDF 参数。
        header.kdf.clamp_to_safe_bounds();
        Ok(Vault {
            root: root.to_path_buf(),
            header,
            mk: None,
        })
    }

    /// 用访问密码解锁。
    pub fn unlock_with_password(&mut self, password: &str) -> Result<()> {
        kdf::ensure_affordable(&self.header.kdf)?;
        let mk = envelope::unwrap_with_password(
            &self.header.envelopes.password,
            password,
            &self.header.kdf,
        )?;
        self.mk = Some(mk);
        Ok(())
    }

    /// 用恢复密钥解锁（忘记密码/换机场景）。
    pub fn unlock_with_recovery(&mut self, recovery_input: &str) -> Result<()> {
        let secret = recovery::parse(recovery_input)?;
        let mk = envelope::unwrap_with_recovery(&self.header.envelopes.recovery, &secret)?;
        self.mk = Some(mk);
        Ok(())
    }

    /// 二次验证：校验访问密码是否正确（不改变解锁状态）。用于查看机密前的重认证。
    pub fn verify_password(&self, password: &str) -> Result<()> {
        kdf::ensure_affordable(&self.header.kdf)?;
        envelope::unwrap_with_password(&self.header.envelopes.password, password, &self.header.kdf)
            .map(|_| ())
    }

    /// 当前 KDF 参数（供设置页展示与移动端兼容性判断）。
    pub fn kdf_params(&self) -> &KdfParams {
        &self.header.kdf
    }

    /// 降低 KDF 参数，让内存受限的设备（手机）也能用访问密码解锁。
    ///
    /// 只用新参数**重新包裹密码信封**（毫秒级），不触碰任何业务数据；
    /// 恢复信封与生物识别信封都不依赖 KDF 参数，因此不受影响。
    ///
    /// 目标参数由调用方标定后传入（便于测试注入快参数，也避免这里再吃一次标定耗时）。
    /// 返回 `None` 表示当前参数已不高于目标、无需改动。
    ///
    /// 注意：改完必须把头部推到云端，其它设备才拿得到新参数。
    pub fn relax_kdf(&mut self, password: &str, target: KdfParams) -> Result<Option<KdfParams>> {
        let mut target = target;
        target.clamp_to_safe_bounds();
        if target.mem_kib > kdf::MEM_CEIL_MOBILE_SAFE_KIB {
            return Err(AppError::Invalid(format!(
                "目标参数 {} MiB 仍高于移动端安全上限 {} MiB",
                target.mem_kib / 1024,
                kdf::MEM_CEIL_MOBILE_SAFE_KIB / 1024
            )));
        }
        if self.header.kdf.mem_kib <= target.mem_kib {
            return Ok(None);
        }
        // 用**旧**参数解开（本机是桌面，跑得动；护栏只挡解锁路径，不挡这里）。
        let mk = envelope::unwrap_with_password(
            &self.header.envelopes.password,
            password,
            &self.header.kdf,
        )?;
        let new_env = envelope::wrap_with_password(&mk, password, &target)?;

        // 落盘失败要回滚内存中的头部，否则会出现"内存已换、磁盘还是旧的"的分叉。
        let old_kdf = std::mem::replace(&mut self.header.kdf, target.clone());
        let old_env = std::mem::replace(&mut self.header.envelopes.password, new_env);
        if let Err(e) = self.persist_header() {
            self.header.kdf = old_kdf;
            self.header.envelopes.password = old_env;
            return Err(e);
        }
        self.mk = Some(mk);
        Ok(Some(target))
    }

    /// 锁定：清零 MK。
    pub fn lock(&mut self) {
        self.mk = None; // MasterKey 的 Drop 会 zeroize
    }

    /// 用已恢复的主密钥解锁（仅供本机 DPAPI 免验证会话）。
    pub fn unlock_with_master_key(&mut self, mk: MasterKey) -> Result<()> {
        self.mk = Some(mk);
        let data_file = self.root.join("data").join("identities.enc");
        if data_file.is_file() && crate::store::load_data(self).is_err() {
            self.lock();
            return Err(AppError::BadPassword);
        }
        Ok(())
    }

    /// 复制主密钥字节（仅用于写入本机免验证会话）。
    pub fn master_key_bytes(&self) -> Result<[u8; crypto::KEY_LEN]> {
        Ok(*self.mk()?.as_bytes())
    }

    pub fn master_key(&self) -> Result<&MasterKey> {
        self.mk()
    }

    pub fn object_name(&self, logical_path: &str) -> Result<String> {
        Ok(self.mk()?.object_name(logical_path))
    }

    fn mk(&self) -> Result<&MasterKey> {
        self.mk.as_ref().ok_or(AppError::Locked)
    }

    /// 派生子密钥（需已解锁）。
    pub fn subkey(&self, label: &[u8]) -> Result<[u8; crypto::KEY_LEN]> {
        Ok(self.mk()?.subkey(label))
    }

    /// 改访问密码：仅用新密码重新包裹 MK（毫秒级，不重加密数据）。
    /// 需先用旧密码校验（要求当前已解锁或提供旧密码）。
    pub fn change_password(&mut self, old_password: &str, new_password: &str) -> Result<()> {
        if new_password.is_empty() {
            return Err(AppError::Invalid("新密码不能为空".into()));
        }
        // 用恢复密钥在手机上解锁后仍可能走到这里，而改密要跑一次 Argon2。
        kdf::ensure_affordable(&self.header.kdf)?;
        // 用旧密码解出 MK（即便已解锁也强制校验旧密码，防越权改密）。
        let mk = envelope::unwrap_with_password(
            &self.header.envelopes.password,
            old_password,
            &self.header.kdf,
        )?;
        let new_env = envelope::wrap_with_password(&mk, new_password, &self.header.kdf)?;
        self.header.envelopes.password = new_env;
        self.persist_header()?;
        // 保持解锁态一致。
        self.mk = Some(mk);
        Ok(())
    }

    /// 轮换恢复密钥：生成新的一把并作废旧的（需已解锁）。返回新展示串。
    pub fn rotate_recovery_key(&mut self) -> Result<String> {
        let mk = self.mk()?.clone();
        let (secret, display) = recovery::generate();
        let new_env = envelope::wrap_with_recovery(&mk, &secret)?;
        self.header.envelopes.recovery = new_env;
        self.persist_header()?;
        Ok(display)
    }

    /// 仅用于云端预览：用已解开的 MK 构造内存中的 Vault（不落盘）。
    pub fn from_header_unlocked(header: VaultHeader, mk: MasterKey) -> Self {
        let mut header = header;
        header.kdf.clamp_to_safe_bounds();
        Vault {
            root: PathBuf::new(),
            header,
            mk: Some(mk),
        }
    }

    /// 换机恢复：沿用旧工作空间头部与 MK，用新访问密码重新包裹后落盘。
    /// 恢复信封保持不变，旧恢复密钥在新机器上仍可解锁。
    pub fn restore(
        root: &Path,
        mut header: VaultHeader,
        mk: MasterKey,
        new_password: &str,
    ) -> Result<Self> {
        if new_password.is_empty() {
            return Err(AppError::Invalid("访问密码不能为空".into()));
        }
        if Self::exists(root) {
            return Err(AppError::AlreadyInitialized(root.display().to_string()));
        }
        std::fs::create_dir_all(root)?;
        for sub in ["data", "keys", "backups", "sync", "ssh-keys", "ssh", "icons"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        header.kdf.clamp_to_safe_bounds();
        header.envelopes.password = envelope::wrap_with_password(&mk, new_password, &header.kdf)?;
        let vault = Vault {
            root: root.to_path_buf(),
            header,
            mk: Some(mk),
        };
        vault.persist_header()?;
        Ok(vault)
    }

    /// 原子写入 vault.json（临时文件 + rename）。
    fn persist_header(&self) -> Result<()> {
        let path = Self::vault_path(&self.root);
        let json = serde_json::to_string_pretty(&self.header)?;
        atomic_write(&path, json.as_bytes())
    }
}

/// 原子写：写临时文件后 rename，避免写一半损坏。
/// Windows 上若目标只读、被短暂锁住或禁止覆盖改名，会清只读、重试，再回退为直接覆盖。
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or_else(|| AppError::Invalid("无效路径".into()))?;
    std::fs::create_dir_all(dir)
        .map_err(|e| AppError::Io(format!("创建目录 {} 失败：{e}", dir.display())))?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("f"),
        uuid::Uuid::new_v4()
    ));
    if let Err(e) = std::fs::write(&tmp, data) {
        return Err(AppError::Io(format!(
            "写入临时文件 {} 失败：{e}",
            tmp.display()
        )));
    }
    if let Err(e) = replace_file_with_tmp(path, &tmp, data) {
        let _ = std::fs::remove_file(&tmp);
        return Err(AppError::Io(format!("写入 {} 失败：{e}", path.display())));
    }
    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

fn make_writable(path: &Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
}

fn replace_file_with_tmp(path: &Path, tmp: &Path, data: &[u8]) -> std::io::Result<()> {
    if path.exists() {
        make_writable(path);
        for attempt in 0..4 {
            match std::fs::remove_file(path) {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(_) if attempt < 3 => {
                    std::thread::sleep(std::time::Duration::from_millis(20 * (attempt as u64 + 1)));
                    make_writable(path);
                }
                Err(_) => return std::fs::write(path, data),
            }
        }
    }
    match std::fs::rename(tmp, path) {
        Ok(()) => Ok(()),
        Err(_) => std::fs::write(path, data),
    }
}

fn now_iso8601() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::kdf::{ITERS_FLOOR, MEM_FLOOR_KIB};

    fn fast_kdf() -> KdfParams {
        KdfParams::new(MEM_FLOOR_KIB, ITERS_FLOOR, 1)
    }

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("gam-test-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn atomic_write_replaces_existing_and_readonly_file() {
        let dir = temp_root();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        std::fs::write(&path, b"old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");

        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms).unwrap();
        atomic_write(&path, b"newer").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"newer");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn relax_kdf_is_noop_when_already_light_enough() {
        let root = temp_root();
        let (mut v, _rec) = Vault::init(&root, "pw", fast_kdf()).unwrap();
        // 目标与当前相同 → 无需改动。
        let got = v.relax_kdf("pw", fast_kdf()).unwrap();
        assert!(got.is_none(), "已经足够轻时不应重写头部");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn relax_kdf_rewraps_password_envelope_and_keeps_recovery() {
        let root = temp_root();
        // 造一个"偏重"的保险库（仍用测试级小参数，只要高于目标即可触发降参）。
        let heavy = KdfParams::new(MEM_FLOOR_KIB * 2, ITERS_FLOOR, 1);
        let (_v, recovery) = Vault::init(&root, "pw", heavy.clone()).unwrap();

        let mut v = Vault::load(&root).unwrap();
        assert_eq!(v.kdf_params().mem_kib, heavy.mem_kib);

        let target = fast_kdf();
        let applied = v.relax_kdf("pw", target.clone()).unwrap().expect("应发生降参");
        assert_eq!(applied.mem_kib, target.mem_kib);
        assert_eq!(v.kdf_params().mem_kib, target.mem_kib);
        assert!(v.is_unlocked(), "降参后应保持解锁态");

        // 重开：新参数已落盘，旧密码仍能解锁。
        let mut reopened = Vault::load(&root).unwrap();
        assert_eq!(reopened.kdf_params().mem_kib, target.mem_kib);
        reopened.unlock_with_password("pw").unwrap();

        // 恢复信封不依赖 KDF 参数，降参不得影响它。
        let mut by_recovery = Vault::load(&root).unwrap();
        by_recovery.unlock_with_recovery(&recovery).unwrap();
        assert!(by_recovery.is_unlocked());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn relax_kdf_rejects_wrong_password_and_too_heavy_target() {
        let root = temp_root();
        let heavy = KdfParams::new(MEM_FLOOR_KIB * 2, ITERS_FLOOR, 1);
        let _ = Vault::init(&root, "pw", heavy).unwrap();
        let mut v = Vault::load(&root).unwrap();

        assert_eq!(
            v.relax_kdf("wrong", fast_kdf()).unwrap_err().code(),
            "BAD_PASSWORD"
        );
        // 目标本身还超出移动端上限 → 拒绝，免得白降一场仍打不开。
        let still_heavy = KdfParams::new(crate::vault::kdf::MEM_CEIL_KIB, ITERS_FLOOR, 1);
        assert_eq!(v.relax_kdf("pw", still_heavy).unwrap_err().code(), "INVALID");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn init_creates_header_without_plaintext_secret() {
        let root = temp_root();
        let (_v, _rec) = Vault::init(&root, "pw123", fast_kdf()).unwrap();
        let raw = std::fs::read_to_string(root.join(VAULT_FILE)).unwrap();
        // 头部里不得出现明文密码或原始 MK 字样；至少确保结构存在。
        assert!(raw.contains("envelopes"));
        assert!(raw.contains("wrappedKey"));
        assert!(!raw.contains("pw123"));
        assert!(Vault::exists(&root));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reopen_and_unlock_with_password() {
        let root = temp_root();
        let (_v, _rec) = Vault::init(&root, "s3cret", fast_kdf()).unwrap();
        // 模拟重开：重新 load（锁定）再解锁。
        let mut reopened = Vault::load(&root).unwrap();
        assert!(!reopened.is_unlocked());
        reopened.unlock_with_password("s3cret").unwrap();
        assert!(reopened.is_unlocked());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn wrong_password_fails() {
        let root = temp_root();
        let _ = Vault::init(&root, "right", fast_kdf()).unwrap();
        let mut v = Vault::load(&root).unwrap();
        assert_eq!(
            v.unlock_with_password("wrong").unwrap_err().code(),
            "BAD_PASSWORD"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn recovery_key_unlocks() {
        let root = temp_root();
        let (_v, rec) = Vault::init(&root, "pw", fast_kdf()).unwrap();
        let mut v = Vault::load(&root).unwrap();
        v.unlock_with_recovery(&rec).unwrap();
        assert!(v.is_unlocked());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn change_password_keeps_data_and_invalidates_old() {
        let root = temp_root();
        let (_v, _rec) = Vault::init(&root, "old", fast_kdf()).unwrap();
        let mut v = Vault::load(&root).unwrap();
        v.unlock_with_password("old").unwrap();
        v.change_password("old", "new").unwrap();

        // 旧密码失效、新密码可用（重新 load 验证已落盘）。
        let mut v2 = Vault::load(&root).unwrap();
        assert!(v2.unlock_with_password("old").is_err());
        assert!(v2.unlock_with_password("new").is_ok());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn change_password_requires_correct_old() {
        let root = temp_root();
        let (_v, _r) = Vault::init(&root, "old", fast_kdf()).unwrap();
        let mut v = Vault::load(&root).unwrap();
        v.unlock_with_password("old").unwrap();
        assert!(v.change_password("bogus", "new").is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rotate_recovery_key_invalidates_old() {
        let root = temp_root();
        let (_v, old_rec) = Vault::init(&root, "pw", fast_kdf()).unwrap();
        let mut v = Vault::load(&root).unwrap();
        v.unlock_with_password("pw").unwrap();
        let new_rec = v.rotate_recovery_key().unwrap();
        assert_ne!(old_rec, new_rec);

        let mut v2 = Vault::load(&root).unwrap();
        assert!(v2.unlock_with_recovery(&old_rec).is_err(), "旧恢复密钥应失效");
        assert!(v2.unlock_with_recovery(&new_rec).is_ok(), "新恢复密钥应可用");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn subkey_requires_unlock() {
        let root = temp_root();
        let _ = Vault::init(&root, "pw", fast_kdf()).unwrap();
        let mut v = Vault::load(&root).unwrap();
        assert!(v.subkey(crypto::LABEL_METADATA).is_err());
        v.unlock_with_password("pw").unwrap();
        assert!(v.subkey(crypto::LABEL_METADATA).is_ok());
        v.lock();
        assert!(!v.is_unlocked());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn double_init_rejected() {
        let root = temp_root();
        let _ = Vault::init(&root, "pw", fast_kdf()).unwrap();
        assert!(Vault::init(&root, "pw", fast_kdf()).is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn restore_keeps_workspace_id_mk_and_recovery() {
        let src = temp_root();
        let (v, rec) = Vault::init(&src, "old-pw", fast_kdf()).unwrap();
        let ws = v.workspace_id().to_string();
        let sync_key = v.subkey(crypto::LABEL_SYNC_OBJECT).unwrap();
        let header = v.header().clone();
        let mk = MasterKey::from_bytes(v.master_key_bytes().unwrap());

        let dest = temp_root();
        let restored = Vault::restore(&dest, header, mk, "new-pw").unwrap();
        assert_eq!(restored.workspace_id(), ws);
        assert_eq!(restored.subkey(crypto::LABEL_SYNC_OBJECT).unwrap(), sync_key);

        let mut by_pw = Vault::load(&dest).unwrap();
        assert!(by_pw.unlock_with_password("old-pw").is_err());
        by_pw.unlock_with_password("new-pw").unwrap();

        let mut by_rec = Vault::load(&dest).unwrap();
        by_rec.unlock_with_recovery(&rec).unwrap();
        assert_eq!(by_rec.subkey(crypto::LABEL_SYNC_OBJECT).unwrap(), sync_key);

        std::fs::remove_dir_all(&src).ok();
        std::fs::remove_dir_all(&dest).ok();
    }
}
