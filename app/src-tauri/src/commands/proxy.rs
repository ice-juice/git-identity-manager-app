//! 网络代理 IPC。

use crate::app_config::NetworkProxy;
use crate::commands::AppState;
use crate::error::Result;
use crate::net;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    pub https_ok: bool,
    pub https_ms: Option<u128>,
    pub https_error: Option<String>,
    pub ssh_helper_found: bool,
    pub ssh_helper_name: Option<String>,
    pub ssh_note: Option<String>,
}

#[tauri::command]
pub fn get_network_proxy(state: State<AppState>) -> Result<Option<NetworkProxy>> {
    let cfg = state.config.lock().unwrap();
    Ok(cfg.network_proxy.clone())
}

#[tauri::command]
pub fn save_network_proxy(state: State<AppState>, proxy: Option<NetworkProxy>) -> Result<()> {
    if let Some(ref p) = proxy {
        if p.enabled {
            let _ = net::proxy_url(p)?;
        } else if !p.host.trim().is_empty() {
            let _ = net::proxy_url(p)?;
        }
    }
    let mut cfg = state.config.lock().unwrap();
    cfg.network_proxy = proxy;
    cfg.save()
}

#[tauri::command]
pub fn test_network_proxy(proxy: NetworkProxy) -> Result<ProxyTestResult> {
    let _ = net::proxy_url(&proxy)?;
    let helper = net::ssh_helper_status();
    let mut ssh_note = None;
    if net::ssh_auth_unsupported(&proxy) {
        ssh_note = Some(
            "SSH 通道暂不支持带用户名密码的代理，请改用本机无认证端口，或只开启 Git HTTPS / 应用内请求。"
                .into(),
        );
    } else if proxy.apply_to_ssh && !helper.found {
        ssh_note = Some(
            "未找到 SSH 代理助手（Git for Windows 的 connect，或 ncat）。HTTPS 仍可用，SSH 克隆/体检不会走代理。"
                .into(),
        );
    } else if proxy.apply_to_ssh && helper.found {
        ssh_note = Some(format!(
            "将使用 {} 为 SSH 注入 ProxyCommand。",
            helper.name.as_deref().unwrap_or("助手")
        ));
    }

    match net::test_github_https(&proxy) {
        Ok(ms) => Ok(ProxyTestResult {
            https_ok: true,
            https_ms: Some(ms),
            https_error: None,
            ssh_helper_found: helper.found,
            ssh_helper_name: helper.name,
            ssh_note,
        }),
        Err(e) => Ok(ProxyTestResult {
            https_ok: false,
            https_ms: None,
            https_error: Some(e.to_string()),
            ssh_helper_found: helper.found,
            ssh_helper_name: helper.name,
            ssh_note,
        }),
    }
}
