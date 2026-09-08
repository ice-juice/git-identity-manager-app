//! M2 命令层：SSH 资产只读视图 + 外部密钥导入。
//! 只读命令直接读系统；导入命令把私钥密文写进已解锁的 vault。

use crate::commands::AppState;
use crate::error::{AppError, Result};
use crate::importer::{import_private_key, ImportRequest};
use crate::model::{Identity, KeyRecord};
use crate::ssh::config::{self, Diagnostic, HostBlock};
use crate::ssh::connect::{self, AuthResult};
use crate::ssh::key::{self, KeyInfo};
use crate::ssh::toolchain::{self, SshBinary, Toolchain};
use crate::sys;
use serde::Serialize;
use std::path::PathBuf;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    pub path: String,
    pub raw: String,
    pub blocks: Vec<HostBlock>,
    pub diagnostics: Vec<Diagnostic>,
}

/// 读取并解析 `~/.ssh/config`，附带只读诊断。
#[tauri::command]
pub fn read_ssh_config() -> Result<ConfigView> {
    let path = sys::ssh_dir().join("config");
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    let cfg = config::parse(&raw);
    let diagnostics = config::diagnose(&cfg, |p| sys::expand_path(p).exists());
    Ok(ConfigView {
        path: path.display().to_string(),
        raw,
        blocks: cfg.blocks,
        diagnostics,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedKey {
    pub path: String,
    pub info: KeyInfo,
    /// 该指纹是否已收编进 vault。
    pub in_vault: bool,
}

/// 扫描 `~/.ssh` 及 config 引用到的外部路径，返回密钥清单。
#[tauri::command]
pub fn scan_keys(state: State<AppState>) -> Result<Vec<ScannedKey>> {
    // 已入库指纹集合（若已解锁）。
    let in_vault: std::collections::HashSet<String> = {
        let vault = state.vault.lock().unwrap();
        match vault.as_ref() {
            Some(v) if v.is_unlocked() => crate::store::load_data(v)
                .map(|d| d.keys.into_iter().map(|k| k.fingerprint).collect())
                .unwrap_or_default(),
            _ => std::collections::HashSet::new(),
        }
    };

    let mut candidates: Vec<PathBuf> = Vec::new();
    // ~/.ssh 下的候选文件。
    if let Ok(rd) = std::fs::read_dir(sys::ssh_dir()) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() {
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name.ends_with(".pub")
                    || name.starts_with("id_")
                    || name.contains("ssh")
                    || name.contains("key")
                {
                    candidates.push(p);
                }
            }
        }
    }
    // config 中引用到的 IdentityFile 外部路径。
    let cfg_path = sys::ssh_dir().join("config");
    if let Ok(text) = std::fs::read_to_string(&cfg_path) {
        for b in config::parse(&text).blocks {
            if let Some(idf) = b.identity_file() {
                let real = sys::expand_path(idf);
                candidates.push(real.clone());
                // 私钥旁的 .pub。
                candidates.push(PathBuf::from(format!("{}.pub", real.display())));
            }
        }
    }

    let mut seen_fp = std::collections::HashSet::new();
    let mut out = Vec::new();
    for path in candidates {
        if !path.is_file() {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let info = if path.extension().and_then(|s| s.to_str()) == Some("pub")
            || text.trim_start().starts_with("ssh-")
            || text.trim_start().starts_with("ecdsa-")
        {
            key::parse_public_openssh(&text)
        } else {
            key::parse_private_openssh(&text)
        };
        if let Ok(info) = info {
            if seen_fp.insert(info.fingerprint.clone()) {
                out.push(ScannedKey {
                    path: path.display().to_string(),
                    in_vault: in_vault.contains(&info.fingerprint),
                    info,
                });
            }
        }
    }
    Ok(out)
}

/// 探测 ssh 工具链（运行 `ssh -V` 解析版本）。
#[tauri::command]
pub fn detect_toolchain() -> Toolchain {
    let mut tc = Toolchain::default();
    for (source, path) in toolchain::candidate_paths() {
        if !path.exists() {
            continue;
        }
        let ps = path.display().to_string();
        let version = sys::run(&ps, &["-V"])
            .ok()
            .and_then(|(o, e, _)| toolchain::parse_ssh_version(if e.trim().is_empty() { &o } else { &e }));
        let bin = SshBinary {
            path: ps,
            version,
            source: source.clone(),
        };
        if source == "system" {
            tc.system = Some(bin);
        } else {
            tc.git = Some(bin);
        }
    }
    // git 实际调用：优先 core.sshCommand / GIT_SSH，否则默认 Git 自带。
    tc.git_uses = detect_git_ssh().or_else(|| tc.git.as_ref().map(|b| b.path.clone()));
    tc
}

fn detect_git_ssh() -> Option<String> {
    if let Ok(v) = std::env::var("GIT_SSH") {
        if !v.is_empty() {
            return Some(v);
        }
    }
    if let Ok((out, _, code)) = sys::run("git", &["config", "--get", "core.sshCommand"]) {
        if code == 0 && !out.trim().is_empty() {
            return Some(out.trim().to_string());
        }
    }
    None
}

/// 列出 vault 中已登记的密钥（需已解锁）。
#[tauri::command]
pub fn list_keys(state: State<AppState>) -> Result<Vec<KeyRecord>> {
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::NotInitialized)?;
    Ok(crate::store::load_data(v)?.keys)
}

/// 列出 vault 中的身份（需已解锁）。
#[tauri::command]
pub fn list_identities(state: State<AppState>) -> Result<Vec<Identity>> {
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::NotInitialized)?;
    Ok(crate::store::load_data(v)?.identities)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportArgs {
    pub private_text: String,
    pub public_text: Option<String>,
    pub passphrase: Option<String>,
    pub source_path: Option<String>,
    pub name: Option<String>,
}

/// 导入外部私钥入库，返回登记的密钥记录（仅公开元数据）。
#[tauri::command]
pub fn import_key(state: State<AppState>, args: ImportArgs) -> Result<KeyRecord> {
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    import_private_key(
        v,
        ImportRequest {
            private_text: args.private_text,
            public_text: args.public_text,
            passphrase: args.passphrase,
            source_path: args.source_path,
            name: args.name,
        },
    )
}

/// 从文件路径导入（读取私钥，自动找同目录 .pub）。
#[tauri::command]
pub fn import_key_from_path(state: State<AppState>, path: String) -> Result<KeyRecord> {
    let private_text = std::fs::read_to_string(&path)?;
    let pub_path = format!("{path}.pub");
    let public_text = std::fs::read_to_string(&pub_path).ok();
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    import_private_key(
        v,
        ImportRequest {
            private_text,
            public_text,
            passphrase: None,
            source_path: Some(path),
            name: None,
        },
    )
}

/// 连接体检：对某个 Host 别名跑 `ssh -T`，解析账号名/错误。
#[tauri::command]
pub fn test_connection(host_alias: String) -> Result<AuthResult> {
    let target = format!("git@{host_alias}");
    let (out, err, _code) = sys::run(
        "ssh",
        &[
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=10",
            &target,
        ],
    )?;
    Ok(connect::parse_auth_result(&out, &err))
}
