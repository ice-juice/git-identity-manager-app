//! 领域模型：身份、密钥记录、机密集合、以及加密落盘的数据容器。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 一把密钥的元数据（公开信息，可展示；私钥密文单独存 keys/<id>.enc）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeyRecord {
    pub id: String,
    pub name: String,
    pub algorithm: String,
    pub fingerprint: String,
    pub public_openssh: String,
    pub bits: Option<u32>,
    pub has_passphrase: bool,
    /// 是否弱密钥（RSA < 2048）。
    pub weak: bool,
    pub source_path: Option<String>,
    pub imported_at: String,
}

/// 一个 Git 身份。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub host_alias: String,
    pub real_host: String,
    pub user: String,
    pub email: Option<String>,
    pub git_user_name: Option<String>,
    pub key_id: Option<String>,
    /// 归属标识（owner 前缀，支持通配 `vortaq-*`）。M5 使用。
    pub owners: Vec<String>,
    /// 严格模式：私钥仅存 agent，磁盘只留 .pub。
    pub strict_mode: bool,
}

/// 加密落盘的元数据容器（data/identities.enc）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VaultData {
    pub identities: Vec<Identity>,
    pub keys: Vec<KeyRecord>,
    /// 自动学习的克隆历史：owner(小写) → identity_id。
    #[serde(default)]
    pub clone_history: HashMap<String, String>,
}

/// 机密集合（data/secrets.enc）——只在 Rust 侧内存出现，绝不跨 IPC。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Secrets {
    /// keyId -> 私钥口令。
    pub key_passphrases: HashMap<String, String>,
    /// GitHub PAT。
    pub github_pat: Option<String>,
}
