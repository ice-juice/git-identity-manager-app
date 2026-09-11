//! Tauri 应用入口：注册状态与命令。

pub mod agent;
pub mod app_config;
pub mod identity;
pub mod autostart;
pub mod biometric;
mod clipboard;
pub mod commands;
pub mod session;
pub mod security;
pub mod error;
pub mod git;
pub mod icons;
pub mod importer;
pub mod model;
pub mod qrscan;
pub mod net;
pub mod platform;
pub mod single_instance;
pub mod ssh;
pub mod store;
pub mod sync;
pub mod sys;
pub mod totp;
pub mod tray;
pub mod update;
pub mod util;
pub mod vault;
pub mod workspace_path;

use commands::AppState;
use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 这些迁移都在动本机 `~/.ssh` 与启动脚本，移动端没有对应物。
    // 注意：`migrate_app_data` 依赖本机配置根目录，移动端要等沙箱路径注入后
    // 才能跑，所以它挪到了 `setup()` 里。
    #[cfg(desktop)]
    {
        // 从访达 / 开始菜单启动时 PATH 往往没有 Homebrew、Git for Windows。
        crate::sys::augment_search_path();
        crate::identity::migrate_app_data();
        crate::sys::migrate_legacy_ssh_names();
        crate::agent::unify::migrate_legacy_scripts();
    }

    #[cfg(windows)]
    {
        // 默认不要 --disable-gpu：白屏根因是主线程堵在 ssh-agent，强制软件渲染只掉帧。
        // 仅当 GAM_DISABLE_GPU=1 时附加，作为兼容性兜底。必须在创建 WebView 之前设置。
        apply_webview2_additional_args();
    }

    // 单实例检测：若已有同款程序在运行，弹窗询问是否通知旧实例锁定保险库后退出。
    // 桌面平台在创建窗口前完成，取消时直接退出、不会闪现界面。
    #[cfg(desktop)]
    if let single_instance::Decision::Exit = single_instance::check() {
        std::process::exit(0);
    }

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init());

    // 应用内自更新只有桌面端有意义；移动端交给应用商店。
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder
        .invoke_handler(tauri::generate_handler![
            commands::vault::vault_status,
            commands::vault::check_workspace_path,
            commands::vault::vault_init,
            commands::vault::vault_unlock,
            commands::vault::vault_unlock_recovery,
            commands::vault::vault_unlock_biometric,
            commands::vault::vault_lock,
            commands::vault::change_password,
            commands::vault::get_kdf_info,
            commands::vault::relax_kdf_for_mobile,
            commands::vault::rotate_recovery_key,
            commands::vault::vault_try_grace_unlock,
            commands::vault::set_launch_at_login,
            commands::vault::set_grace_days,
            commands::vault::factory_reset,
            commands::biometric::biometric_status,
            commands::biometric::biometric_enable,
            commands::biometric::biometric_disable,
            commands::biometric::reveal_authorize_biometric,
            commands::biometric::set_biometric_reveal_enabled,
            commands::biometric::set_biometric_reveal_secret,
            commands::security::security_checklist,
            commands::assets::read_ssh_config,
            commands::assets::open_ssh_config,
            commands::assets::scan_keys,
            commands::assets::detect_toolchain,
            commands::assets::list_keys,
            commands::assets::list_identities,
            commands::assets::workspace_nav_counts,
            commands::assets::import_key,
            commands::assets::import_key_from_path,
            commands::assets::test_connection,
            commands::assets::open_url,
            commands::write::generate_key,
            commands::write::preview_config,
            commands::write::apply_config,
            commands::write::create_identity,
            commands::write::stage_identity_draft,
            commands::write::abort_identity_draft,
            commands::write::update_identity,
            commands::write::delete_identity,
            commands::write::reveal_key_passphrase,
            commands::write::reveal_key_material,
            commands::agent::agent_status,
            commands::agent::agent_ensure,
            commands::agent::agent_unify_env,
            commands::agent::agent_load,
            commands::agent::agent_load_identity,
            commands::agent::agent_load_all,
            commands::agent::agent_unload,
            commands::agent::agent_clear,
            commands::repo::resolve_url,
            commands::repo::scan_repos,
            commands::repo::scan_and_import_repos,
            commands::repo::list_managed_repos,
            commands::repo::remove_managed_repo,
            commands::repo::set_repo_remote,
            commands::repo::open_repo_dir,
            commands::repo::add_owner,
            commands::repo::switch_repo_identity,
            commands::repo::inspect_clone_target,
            commands::repo::clone_repo,
            commands::repo::github_pat_status,
            commands::repo::set_github_pat,
            commands::repo::clear_github_pat,
            commands::repo::test_github_pat,
            commands::repo::list_github_orgs,
            commands::repo::upload_public_key,
            commands::sync::export_vault_backup,
            commands::sync::inspect_vault_backup,
            commands::sync::import_vault_backup,
            commands::sync::get_cloud_sync_config,
            commands::sync::save_cloud_sync_config,
            commands::sync::export_s3_config,
            commands::sync::import_s3_config,
            commands::sync::test_cloud_sync_config,
            commands::sync::get_cloud_sync_status,
            commands::sync::get_cloud_sync_page,
            commands::sync::cloud_sync_push,
            commands::sync::cloud_sync_pull,
            commands::sync::get_auto_sync_settings,
            commands::sync::set_auto_sync_minutes,
            commands::sync::list_cloud_snapshots,
            commands::sync::restore_cloud_snapshot,
            commands::sync::run_auto_sync_now,
            commands::sync::preview_cloud_restore,
            commands::sync::restore_from_cloud,
            commands::window::apply_close_choice,
            commands::window::get_close_preference,
            commands::window::clear_close_preference,
            commands::update::get_update_source,
            commands::update::save_update_source,
            commands::update::get_auto_check_update,
            commands::update::set_auto_check_update,
            commands::update::check_update,
            commands::update::download_and_install_update,
            commands::update::skip_update_version,
            commands::update::get_last_update_check,
            commands::proxy::get_network_proxy,
            commands::proxy::save_network_proxy,
            commands::proxy::test_network_proxy,
            commands::totp::totp_list,
            commands::totp::totp_add,
            commands::totp::totp_update,
            commands::totp::totp_delete,
            commands::totp::totp_save_groups,
            commands::totp::totp_generate_code,
            commands::totp::totp_parse_uri,
            commands::totp::totp_import_from_image,
            commands::totp::totp_scan_screen,
            commands::totp::totp_reveal_secret,
            commands::totp::totp_export_qr,
            commands::accounts::account_list,
            commands::accounts::account_add,
            commands::accounts::account_update,
            commands::accounts::account_delete,
            commands::accounts::account_save_groups,
            commands::accounts::account_reveal_password,
            commands::accounts::account_touch,
            commands::accounts::account_history_list,
            commands::accounts::account_reveal_history,
            commands::accounts::account_rollback_history,
            commands::accounts::account_clear_history,
            commands::secrets_ui::clipboard_write,
            commands::secrets_ui::clipboard_clear,
            commands::secrets_ui::get_reveal_settings,
            commands::secrets_ui::set_reveal_grace_minutes,
            commands::secrets_ui::set_clipboard_clear_seconds,
            commands::secrets_ui::set_account_history_limit,
            commands::secrets_ui::icon_list_builtin,
            commands::secrets_ui::icon_upload_custom,
            commands::secrets_ui::icon_get_custom,
        ])
        .setup(|app| {
            // 移动端的保险库目录只能问 Tauri 的 path API（Android 要走 Context）。
            // 必须在 `AppState::new()`（内部会 `AppConfig::load()`）之前注入，
            // 否则会退化到 `std::env::temp_dir()`，保险库可能被系统清空。
            #[cfg(mobile)]
            {
                let dir = app
                    .path()
                    .app_data_dir()
                    .expect("移动端必须能解析应用私有数据目录");
                std::fs::create_dir_all(&dir).ok();
                crate::identity::init_base_dir(dir);
                crate::identity::migrate_app_data();
            }

            // 桌面端在 `run()` 开头已经迁移过，这里只负责注册状态。
            app.manage(AppState::new());

            #[cfg(desktop)]
            {
                tray::install(app.handle())?;
                let handle = app.handle().clone();
                crate::single_instance::on_ready(move || {
                    let state = handle.state::<AppState>();
                    crate::commands::window::quit_app(&handle, &state);
                });
                crate::update::scheduler::start(app.handle().clone());
                commands::agent::bootstrap_git_agent(app.handle());
            }
            crate::sync::scheduler::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // 移动端没有"关闭窗口"这回事，托盘/最小化也不存在。
            #[cfg(mobile)]
            {
                let _ = (window, event);
            }
            #[cfg(desktop)]
            {
                if window.label() != "main" {
                    return;
                }
                let tauri::WindowEvent::CloseRequested { api, .. } = event else {
                    return;
                };
                let state = window.state::<AppState>();
                if state.allow_exit.load(Ordering::SeqCst) {
                    return;
                }
                let action = commands::recover_lock(&state.config).close_action.clone();
                match action.as_deref() {
                    Some("quit") => {}
                    Some("tray") => {
                        api.prevent_close();
                        crate::commands::vault::lock_in_memory(&state);
                        let _ = window.hide();
                    }
                    _ => {
                        api.prevent_close();
                        let _ = window.emit("close-requested", ());
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, event| {
            // 应用退出时释放单实例锁（各退出路径最终都会触发 Exit）
            if let tauri::RunEvent::Exit = event {
                #[cfg(desktop)]
                crate::single_instance::release_lock();
            }
        });
}

/// 仅 `GAM_DISABLE_GPU=1` 时附加软件渲染参数；debug 保留远程调试端口。
fn webview2_extra_browser_args(disable_gpu: bool, debug: bool) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if disable_gpu {
        parts.push("--disable-gpu");
        parts.push("--disable-gpu-compositing");
    }
    if debug {
        parts.push("--remote-debugging-port=9222");
    }
    parts.join(" ")
}

#[cfg(windows)]
fn apply_webview2_additional_args() {
    let extra = webview2_extra_browser_args(
        matches!(std::env::var("GAM_DISABLE_GPU").ok().as_deref(), Some("1")),
        cfg!(debug_assertions),
    );
    if extra.is_empty() {
        return;
    }
    match std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") {
        Ok(existing) if !existing.trim().is_empty() => {
            let mut merged = existing;
            for flag in extra.split_whitespace() {
                if !merged.split_whitespace().any(|e| e == flag) {
                    merged.push(' ');
                    merged.push_str(flag);
                }
            }
            std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", merged);
        }
        _ => std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", extra),
    }
}

#[cfg(test)]
mod tests {
    use super::webview2_extra_browser_args;

    #[test]
    fn webview2_args_default_has_no_disable_gpu() {
        assert_eq!(webview2_extra_browser_args(false, false), "");
        assert_eq!(
            webview2_extra_browser_args(false, true),
            "--remote-debugging-port=9222"
        );
        assert!(!webview2_extra_browser_args(false, true).contains("--disable-gpu"));
    }

    #[test]
    fn webview2_args_gpu_only_when_requested() {
        assert_eq!(
            webview2_extra_browser_args(true, false),
            "--disable-gpu --disable-gpu-compositing"
        );
        assert_eq!(
            webview2_extra_browser_args(true, true),
            "--disable-gpu --disable-gpu-compositing --remote-debugging-port=9222"
        );
    }
}
