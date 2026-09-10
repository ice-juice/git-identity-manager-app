//! M2 命令层：SSH 资产只读视图 + 外部密钥导入。
//! 只读命令直接读系统；导入命令把私钥密文写进已解锁的 vault。

use crate::commands::AppState;
use crate::error::{AppError, Result};
use crate::importer::{import_private_key, ImportRequest};
use crate::model::{Identity, KeyRecord};
use crate::ssh::config::{self, Diagnostic, HostBlock, Severity};
use crate::ssh::connect::{self, AuthResult};
use crate::ssh::key::{self, KeyInfo};
use crate::ssh::toolchain::{self, SshBinary, Toolchain};
use crate::sys;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, State};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    pub path: String,
    pub workspace_path: Option<String>,
    pub system_path: String,
    pub registered: bool,
    pub raw: String,
    pub blocks: Vec<HostBlock>,
    pub diagnostics: Vec<Diagnostic>,
}

fn workspace_root(state: &AppState) -> Option<std::path::PathBuf> {
    if let Ok(vault) = state.vault.lock() {
        if let Some(v) = vault.as_ref() {
            return Some(v.root().to_path_buf());
        }
    }
    state
        .config
        .lock()
        .ok()
        .and_then(|c| c.workspace_path.clone())
        .map(std::path::PathBuf::from)
}

/// 读取工作空间 SSH config 正本（不是 ~/.ssh/config 的 Include 入口）。
#[tauri::command]
pub fn read_ssh_config(state: State<AppState>) -> Result<ConfigView> {
    let root = workspace_root(&state).ok_or_else(|| AppError::Invalid("尚未设置工作空间".into()))?;
    {
        let vault = state.vault.lock().unwrap();
        if let Some(v) = vault.as_ref() {
            if v.is_unlocked() && !state.writes_locked.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = crate::commands::write::reconcile_ssh_hosts(v);
            }
        }
    }
    let (path, raw) = sys::read_workspace_ssh_config(&root);
    let _ = sys::ensure_home_ssh_bridge(&root);
    let cfg = config::parse(&raw);
    let mut diagnostics = config::diagnose(&cfg, |p| sys::expand_path(p).exists());
    if sys::text_is_include_only(&raw) {
        diagnostics.insert(
            0,
            Diagnostic {
                severity: Severity::Error,
                code: "workspace-config-is-stub".into(),
                message: "工作空间 ssh/config 被写成了 Include 入口，没有 Host。已尝试按身份重建，请点刷新。".into(),
                host: None,
            },
        );
    }
    let system_path = sys::home_ssh_config();
    let dest = sys::workspace_ssh_config(&root);
    let home = std::fs::read_to_string(&system_path).unwrap_or_default();
    let registered = sys::is_system_include_stub(&home, &dest);
    Ok(ConfigView {
        path: path.display().to_string(),
        workspace_path: Some(path.display().to_string()),
        system_path: system_path.display().to_string(),
        registered,
        raw,
        blocks: cfg.blocks,
        diagnostics,
    })
}

/// 用记事本打开工作空间内的 SSH config 正本。
#[tauri::command]
pub fn open_ssh_config(state: State<AppState>) -> Result<String> {
    crate::commands::ensure_writes_allowed(&state)?;
    let ws = state
        .config
        .lock()
        .unwrap()
        .workspace_path
        .clone()
        .ok_or_else(|| AppError::Invalid("尚未设置工作空间".into()))?;
    let dest = sys::adopt_ssh_config(std::path::Path::new(&ws))?;
    let path = dest.display().to_string();
    std::process::Command::new("notepad")
        .arg(&path)
        .spawn()
        .map_err(|e| AppError::Io(format!("无法启动记事本：{e}")))?;
    Ok(path)
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
    // config 中引用到的 IdentityFile 外部路径（读工作空间正本）。
    let ws = state.config.lock().unwrap().workspace_path.clone();
    let (_, text) = sys::read_canonical_ssh_config(ws.as_deref().map(std::path::Path::new));
    if !text.is_empty() {
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
    // Unix 没有 Git for Windows 那套路径；把本机 / Homebrew OpenSSH 记为首选。
    if tc.git.is_none() {
        if let Some(path) = toolchain::find_ssh_tool("ssh") {
            let ps = path.display().to_string();
            let version = sys::run(&ps, &["-V"])
                .ok()
                .and_then(|(o, e, _)| toolchain::parse_ssh_version(if e.trim().is_empty() { &o } else { &e }));
            tc.git = Some(SshBinary {
                path: ps,
                version,
                source: toolchain::preferred_ssh_source().into(),
            });
        }
    }
    // git 实际调用：优先 core.sshCommand / GIT_SSH，否则首选 ssh。
    tc.git_uses = detect_git_ssh()
        .or_else(|| tc.git.as_ref().map(|b| b.path.clone()))
        .or_else(|| tc.system.as_ref().map(|b| b.path.clone()));
    tc
}

fn detect_git_ssh() -> Option<String> {
    if let Ok(v) = std::env::var("GIT_SSH") {
        if !v.is_empty() {
            return Some(v);
        }
    }
    if let Ok((out, _, code)) = sys::run_git(&["config", "--get", "core.sshCommand"]) {
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
    let root = v.root().to_path_buf();
    let mut keys = crate::store::load_data(v)?.keys;
    for key in &mut keys {
        if let Some(p) = &key.deployed_path {
            key.deployed_path = Some(sys::resolve_identity_file(p, &root));
        }
    }
    Ok(keys)
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
pub fn import_key(app: AppHandle, state: State<AppState>, args: ImportArgs) -> Result<KeyRecord> {
    crate::commands::ensure_writes_allowed(&state)?;
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    let rec = import_private_key(
        v,
        ImportRequest {
            private_text: args.private_text,
            public_text: args.public_text,
            passphrase: args.passphrase,
            source_path: args.source_path,
            name: args.name,
        },
    )?;
    drop(vault);
    crate::sync::scheduler::kick_publish(app);
    Ok(rec)
}

/// 从文件路径导入（读取私钥，自动找同目录 .pub）。
#[tauri::command]
pub fn import_key_from_path(app: AppHandle, state: State<AppState>, path: String) -> Result<KeyRecord> {
    crate::commands::ensure_writes_allowed(&state)?;
    let private_text = std::fs::read_to_string(&path)?;
    let pub_path = format!("{path}.pub");
    let public_text = std::fs::read_to_string(&pub_path).ok();
    let vault = state.vault.lock().unwrap();
    let v = vault.as_ref().ok_or(AppError::Locked)?;
    let rec = import_private_key(
        v,
        ImportRequest {
            private_text,
            public_text,
            passphrase: None,
            source_path: Some(path),
            name: None,
        },
    )?;
    drop(vault);
    crate::sync::scheduler::kick_publish(app);
    Ok(rec)
}

/// 连接体检：对某个 Host 别名跑 `ssh -T`，解析账号名/错误。
#[tauri::command]
pub fn test_connection(state: State<AppState>, host_alias: String) -> Result<AuthResult> {
    let env = {
        let current = state.agent_env.lock().unwrap().clone();
        if crate::agent::is_ready(&current) {
            current
        } else {
            match crate::agent::ensure() {
                Ok(started) => {
                    *state.agent_env.lock().unwrap() = started.clone();
                    started
                }
                Err(e) => return Err(e),
            }
        }
    };
    let ssh = crate::agent::ssh_bin(&env);
    let target = format!("git@{host_alias}");
    let config_file = workspace_root(&state).and_then(|root| {
        let _ = sys::ensure_home_ssh_bridge(&root);
        let ws = sys::workspace_ssh_config(&root);
        if ws.is_file() {
            Some(ws.to_string_lossy().replace('\\', "/"))
        } else {
            let home = sys::home_included_config();
            home.is_file().then(|| home.to_string_lossy().replace('\\', "/"))
        }
    });
    let mut cmd = std::process::Command::new(&ssh);
    crate::sys::hide_console(&mut cmd);
    if let Some(cfg) = &config_file {
        cmd.args(["-F", cfg]);
    }
    cmd.args([
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-o",
        "ConnectTimeout=10",
        &target,
    ]);
    crate::agent::apply_to_command(&mut cmd, &env);
    if let Some(p) = crate::net::effective(&state.config.lock().unwrap()) {
        crate::net::apply_ssh_command(&mut cmd, &p)?;
    }
    let output = cmd
        .output()
        .map_err(|e| AppError::Io(format!("执行 ssh 失败：{e}")))?;
    let out = String::from_utf8_lossy(&output.stdout).to_string();
    let err = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(connect::parse_auth_result(&out, &err))
}

/// 用系统默认浏览器打开 http(s) 链接（例如 GitHub SSH 设置页）。
#[tauri::command]
pub fn open_url(url: String) -> Result<()> {
    let t = url.trim();
    if !(t.starts_with("https://") || t.starts_with("http://")) {
        return Err(AppError::Invalid("仅允许打开 http(s) 链接".into()));
    }
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", t])
            .spawn()
            .map_err(|e| AppError::Io(format!("打开链接失败：{e}")))?;
    }
    #[cfg(not(windows))]
    {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        std::process::Command::new(opener)
            .arg(t)
            .spawn()
            .map_err(|e| AppError::Io(format!("打开链接失败：{e}")))?;
    }
    Ok(())
}
