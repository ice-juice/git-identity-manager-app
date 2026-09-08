//! `vault.json` 头部结构：版本、KDF 参数、两个信封。**不含任何明文机密**。

use crate::vault::kdf::{ITERS_FLOOR, MEM_CEIL_KIB, MEM_FLOOR_KIB};
use serde::{Deserialize, Serialize};

pub const VAULT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KdfParams {
    pub algo: String,
    #[serde(rename = "memKiB")]
    pub mem_kib: u32,
    pub iters: u32,
    pub parallelism: u32,
}

impl KdfParams {
    pub fn new(mem_kib: u32, iters: u32, parallelism: u32) -> Self {
        KdfParams {
            algo: "argon2id".to_string(),
            mem_kib,
            iters,
            parallelism,
        }
    }

    /// 收敛到安全边界，防止头部被篡改成弱参数或非法参数。
    pub fn clamp_to_safe_bounds(&mut self) {
        self.mem_kib = self.mem_kib.clamp(MEM_FLOOR_KIB, MEM_CEIL_KIB);
        self.iters = self.iters.max(ITERS_FLOOR);
        self.parallelism = self.parallelism.clamp(1, 8);
        // Argon2 要求 mem >= 8 * parallelism（KiB），此处已远超。
        if self.mem_kib < 8 * self.parallelism {
            self.mem_kib = 8 * self.parallelism;
        }
    }
}

/// 一个信封：用某来源派生的 KEK 包裹主密钥 MK 的结果（全部 base64）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub salt: String,
    pub nonce: String,
    #[serde(rename = "wrappedKey")]
    pub wrapped_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelopes {
    pub password: Envelope,
    pub recovery: Envelope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultHeader {
    pub version: u32,
    #[serde(rename = "workspaceId")]
    pub workspace_id: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub kdf: KdfParams,
    pub envelopes: Envelopes,
}
