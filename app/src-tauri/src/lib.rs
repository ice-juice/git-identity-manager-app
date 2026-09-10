//! Tauri 应用入口：注册状态与命令。

pub mod agent;
pub mod app_config;
pub mod autostart;
pub mod commands;
pub mod session;
pub mod error;
pub mod git;
pub mod importer;
pub mod model;
pub mod platform;
pub mod single_instance;
pub mod ssh;
pub mod store;
pub mod sync;
pub mod sys;
pub mod tray;
pub mod update;
pub mod util;
pub mod vault;

use commands::AppState;
use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 单实例检测：若已有同款程序在运行，弹窗询问是否通知旧实例锁定保险库后退出。
    // 桌面平台在创建窗口前完成，取消时直接退出、不会闪现界面。
    #[cfg(desktop)]
    if let single_instance::Decision::Exit = single_instance::check() {
        std::process::exit(0);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::vault::vault_status,
            commands::vault::check_workspace_path,
            commands::vault::vault_init,
            commands::vault::vault_unlock,
            commands::vault::vault_unlock_recovery,
            commands::vault::vault_lock,
            commands::vault::change_password,
            commands::vault::rotate_recovery_key,
            commands::vault::vault_try_grace_unlock,
            commands::vault::set_launch_at_login,
            commands::vault::set_grace_days,
            commands::vault::factory_reset,
            commands::assets::read_ssh_config,
            commands::assets::open_ssh_config,
            commands::assets::scan_keys,
            commands::assets::detect_toolchain,
            commands::assets::list_keys,
            commands::assets::list_identities,
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
        ])
        .setup(|app| {
            tray::install(app.handle())?;
            #[cfg(desktop)]
            {
                let handle = app.handle().clone();
                crate::single_instance::on_ready(move || {
                    let state = handle.state::<AppState>();
                    crate::commands::window::quit_app(&handle, &state);
                });
            }
            crate::sync::scheduler::start(app.handle().clone());
            crate::update::scheduler::start(app.handle().clone());
            commands::agent::bootstrap_git_agent(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
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
            let action = state.config.lock().unwrap().close_action.clone();
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
