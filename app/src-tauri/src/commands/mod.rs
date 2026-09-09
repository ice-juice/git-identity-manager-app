//! IPC 命令层：薄封装，仅做参数校验 + 编排 + 结果映射。
//! **绝不在此写机密逻辑，绝不把私钥/口令/MK 传回前端。**

pub mod agent;
pub mod assets;
pub mod repo;
pub mod sync;
pub mod update;
pub mod vault;
pub mod window;
pub mod write;

use crate::app_config::AppConfig;
use crate::vault::Vault;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Instant;

/// 解锁限速：连续失败递增延迟，防手动试探。
#[derive(Default)]
pub struct UnlockGuard {
    pub fails: u32,
    pub blocked_until: Option<Instant>,
}

impl UnlockGuard {
    /// 返回剩余冷却毫秒；0 表示可尝试。
    pub fn remaining_ms(&self) -> u128 {
        match self.blocked_until {
            Some(t) => {
                let now = Instant::now();
                if now < t {
                    (t - now).as_millis()
                } else {
                    0
                }
            }
            None => 0,
        }
    }

    pub fn record_failure(&mut self) {
        self.fails += 1;
        // 1s,2s,4s,8s... 上限 30s；10 次后固定 30s 冷却。
        let secs = if self.fails >= 10 {
            30
        } else {
            (1u64 << (self.fails.min(5) - 1)).min(30)
        };
        self.blocked_until = Some(Instant::now() + std::time::Duration::from_secs(secs));
    }

    pub fn reset(&mut self) {
        self.fails = 0;
        self.blocked_until = None;
    }
}

/// 全局应用状态。
pub struct AppState {
    pub vault: Mutex<Option<Vault>>,
    pub config: Mutex<AppConfig>,
    pub unlock_guard: Mutex<UnlockGuard>,
    /// 免提权 fallback 启动的 agent 环境（sock/pid）。
    pub agent_env: Mutex<crate::agent::AgentEnv>,
    /// 用户已确认退出时放行 CloseRequested，避免再次弹出确认框。
    pub allow_exit: AtomicBool,
    /// 自动同步进行中，避免重叠拉取/推送。
    pub auto_sync_busy: AtomicBool,
    /// 最近一次周期同步完成时刻（进程内）。
    pub last_periodic_sync: Mutex<Option<Instant>>,
    /// 编辑后若当时正在同步，结束后再推一次。
    pub pending_edit_publish: AtomicBool,
    /// 启动后首次云同步未完成时禁止写入。
    pub writes_locked: AtomicBool,
    /// 后台启动任务进行中，避免重叠。
    pub bootstrap_busy: AtomicBool,
    /// 给界面看的启动阶段说明。
    pub startup_note: Mutex<Option<String>>,
    /// 更新下载/安装进行中，避免重叠。
    pub update_busy: AtomicBool,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            vault: Mutex::new(None),
            config: Mutex::new(AppConfig::load()),
            unlock_guard: Mutex::new(UnlockGuard::default()),
            agent_env: Mutex::new(crate::agent::AgentEnv::default()),
            allow_exit: AtomicBool::new(false),
            auto_sync_busy: AtomicBool::new(false),
            last_periodic_sync: Mutex::new(None),
            pending_edit_publish: AtomicBool::new(false),
            writes_locked: AtomicBool::new(false),
            bootstrap_busy: AtomicBool::new(false),
            startup_note: Mutex::new(None),
            update_busy: AtomicBool::new(false),
        }
    }
}

pub fn ensure_writes_allowed(state: &AppState) -> crate::error::Result<()> {
    if state.writes_locked.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(crate::error::AppError::Busy);
    }
    Ok(())
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlock_guard_backs_off_and_resets() {
        let mut g = UnlockGuard::default();
        assert_eq!(g.remaining_ms(), 0, "初始可尝试");
        g.record_failure();
        assert_eq!(g.fails, 1);
        assert!(g.remaining_ms() > 0, "失败后进入冷却");
        g.record_failure();
        assert_eq!(g.fails, 2);
        // 成功后清零。
        g.reset();
        assert_eq!(g.fails, 0);
        assert_eq!(g.remaining_ms(), 0);
    }

    #[test]
    fn unlock_guard_caps_after_many_failures() {
        let mut g = UnlockGuard::default();
        for _ in 0..12 {
            g.record_failure();
        }
        assert_eq!(g.fails, 12);
        // 达上限后仍处于冷却（30s 档）。
        assert!(g.remaining_ms() > 0);
    }
}
