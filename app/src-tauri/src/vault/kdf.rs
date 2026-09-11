//! Argon2id 密码派生 + 运行时参数标定。
//!
//! 目标：把一次密码派生标定到约 350–700ms（默认目标 500ms），
//! 内存上限 512MiB，下限 19MiB / 迭代 2（OWASP 兜底）。参数写入 vault.json 头部。

use crate::error::{AppError, Result};
use crate::vault::crypto::KEY_LEN;
use crate::vault::header::KdfParams;
use std::time::Instant;

pub const MEM_FLOOR_KIB: u32 = 19 * 1024; // 19 MiB
pub const MEM_CEIL_KIB: u32 = 512 * 1024; // 512 MiB
pub const ITERS_FLOOR: u32 = 2;
pub const DEFAULT_TARGET_MS: u128 = 500;

/// 移动端可安全承受的 Argon2 内存上限（KiB）。
///
/// Android 低内存机型一次性申请超过这个量的**原生**内存会被 LMK 直接杀掉，
/// iOS 会触发 Jetsam；两者在用户看来都是「闪退」，既无法理解也无法自救。
///
/// 要紧的是 KDF 参数写在 `vault.json` 头部、并经云同步共享给**所有**设备，
/// 所以这不是"手机上换个参数"能解决的：新建保险库必须默认收在这个上限内，
/// 强度用提高 `iters` 来补（Argon2id 128MiB + 更多轮次远高于 OWASP 建议值）。
pub const MEM_CEIL_MOBILE_SAFE_KIB: u32 = 128 * 1024; // 128 MiB

/// 标定档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdfProfile {
    /// 跨设备兼容（默认）：内存上限 128MiB，手机也能解锁。
    CrossDevice,
    /// 仅桌面：内存上限 512MiB。强度更高，但手机可能永远打不开这个保险库。
    DesktopOnly,
}

impl KdfProfile {
    pub fn mem_ceiling_kib(self) -> u32 {
        match self {
            KdfProfile::CrossDevice => MEM_CEIL_MOBILE_SAFE_KIB,
            KdfProfile::DesktopOnly => MEM_CEIL_KIB,
        }
    }
}

/// 本机能安全承受的内存上限。
pub fn local_mem_ceiling_kib() -> u32 {
    #[cfg(mobile)]
    {
        MEM_CEIL_MOBILE_SAFE_KIB
    }
    #[cfg(not(mobile))]
    {
        MEM_CEIL_KIB
    }
}

/// 派生前的护栏。
///
/// 超过本机安全上限时给出**可理解的错误**，而不是硬跑到被系统杀掉——
/// 后者在移动端表现为闪退，用户只会以为程序坏了。
/// 只用于访问密码路径；恢复密钥走 HKDF，不吃内存，无需护栏。
pub fn ensure_affordable(p: &KdfParams) -> Result<()> {
    let ceiling = local_mem_ceiling_kib();
    if p.mem_kib > ceiling {
        return Err(AppError::KdfTooHeavy {
            needed_mib: p.mem_kib / 1024,
            ceiling_mib: ceiling / 1024,
        });
    }
    Ok(())
}

/// 用给定参数把密码派生成 32 字节 KEK。
pub fn derive_kek(password: &[u8], salt: &[u8], p: &KdfParams) -> Result<[u8; KEY_LEN]> {
    let params = argon2::Params::new(p.mem_kib, p.iters, p.parallelism, Some(KEY_LEN))
        .map_err(|_| AppError::Crypto)?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = [0u8; KEY_LEN];
    argon
        .hash_password_into(password, salt, &mut out)
        .map_err(|_| AppError::Crypto)?;
    Ok(out)
}

fn parallelism() -> u32 {
    std::thread::available_parallelism()
        .map(|n| (n.get() as u32).clamp(1, 4))
        .unwrap_or(1)
}

fn measure_ms(p: &KdfParams) -> u128 {
    let salt = [0u8; 16];
    let pw = b"calibration-probe";
    let t = Instant::now();
    let _ = derive_kek(pw, &salt, p);
    t.elapsed().as_millis()
}

/// 运行时标定：在内存/迭代维度上逼近目标耗时，返回落在安全边界内的参数。
///
/// 默认走 [`KdfProfile::CrossDevice`]，即新建的保险库手机也能打开。
pub fn calibrate(target_ms: u128) -> KdfParams {
    calibrate_with(target_ms, KdfProfile::CrossDevice)
}

/// 按指定档位标定。
pub fn calibrate_with(target_ms: u128, profile: KdfProfile) -> KdfParams {
    let par = parallelism();
    let ceiling = profile.mem_ceiling_kib();
    // 从 64MiB / 2 次起步（保证不低于兜底），先加内存再加迭代。
    let mut mem = (64u32 * 1024).min(ceiling);
    let mut iters = ITERS_FLOOR;

    // 阶段一：抬内存到档位上限或首次达标。
    loop {
        let p = KdfParams::new(mem, iters, par);
        if measure_ms(&p) >= target_ms || mem >= ceiling {
            break;
        }
        mem = (mem * 2).min(ceiling);
    }
    // 阶段二：内存封顶后，加迭代逼近目标。
    while measure_ms(&KdfParams::new(mem, iters, par)) < target_ms && iters < 64 {
        iters += 1;
    }

    let mut result = KdfParams::new(mem, iters, par);
    result.clamp_to_safe_bounds();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_is_deterministic() {
        let p = KdfParams::new(MEM_FLOOR_KIB, ITERS_FLOOR, 1);
        let salt = [1u8; 16];
        let a = derive_kek(b"pw", &salt, &p).unwrap();
        let b = derive_kek(b"pw", &salt, &p).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_password_or_salt_differs() {
        let p = KdfParams::new(MEM_FLOOR_KIB, ITERS_FLOOR, 1);
        let s1 = [1u8; 16];
        let s2 = [2u8; 16];
        let a = derive_kek(b"pw", &s1, &p).unwrap();
        let b = derive_kek(b"pw2", &s1, &p).unwrap();
        let c = derive_kek(b"pw", &s2, &p).unwrap();
        assert_ne!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn mobile_ceiling_is_below_desktop_ceiling() {
        assert!(MEM_CEIL_MOBILE_SAFE_KIB < MEM_CEIL_KIB);
        assert!(MEM_CEIL_MOBILE_SAFE_KIB >= MEM_FLOOR_KIB, "不能低于 OWASP 兜底");
        assert_eq!(KdfProfile::CrossDevice.mem_ceiling_kib(), MEM_CEIL_MOBILE_SAFE_KIB);
        assert_eq!(KdfProfile::DesktopOnly.mem_ceiling_kib(), MEM_CEIL_KIB);
    }

    #[test]
    fn cross_device_calibration_never_exceeds_mobile_ceiling() {
        // 用极小目标让测试快速返回；关键是上限不被突破。
        let p = calibrate_with(1, KdfProfile::CrossDevice);
        assert!(
            p.mem_kib <= MEM_CEIL_MOBILE_SAFE_KIB,
            "跨设备档位标定出了手机扛不住的 {} KiB",
            p.mem_kib
        );
        // 默认入口必须就是跨设备档位，否则新建的保险库手机打不开。
        assert!(calibrate(1).mem_kib <= MEM_CEIL_MOBILE_SAFE_KIB);
    }

    #[test]
    fn affordability_guard_rejects_only_over_ceiling() {
        let ceiling = local_mem_ceiling_kib();
        let ok = KdfParams::new(ceiling, ITERS_FLOOR, 1);
        assert!(ensure_affordable(&ok).is_ok(), "正好等于上限应放行");

        // 构造一个必然超限的参数（绕过 clamp，直接造结构体）。
        let heavy = KdfParams::new(ceiling.saturating_add(1024), ITERS_FLOOR, 1);
        match ensure_affordable(&heavy) {
            Err(e) => assert_eq!(e.code(), "KDF_TOO_HEAVY"),
            Ok(()) => panic!("超过本机上限必须拒绝，不能硬跑到被系统杀进程"),
        }
    }

    /// 回归护栏：`clamp_to_safe_bounds` 的上限一旦被收紧到移动端安全值，
    /// 历史上用 256/512MiB 创建的保险库会算出不同 KEK，变成永久打不开。
    #[test]
    fn clamp_preserves_legacy_heavy_params() {
        let mut p = KdfParams::new(512 * 1024, 3, 4);
        p.clamp_to_safe_bounds();
        assert_eq!(p.mem_kib, 512 * 1024, "不得把历史重参数 clamp 下来");

        let mut p = KdfParams::new(256 * 1024, 3, 4);
        p.clamp_to_safe_bounds();
        assert_eq!(p.mem_kib, 256 * 1024);
    }

    #[test]
    fn calibrate_returns_bounded_params() {
        // 用极小目标让测试快速返回。
        let p = calibrate(1);
        assert!(p.mem_kib >= MEM_FLOOR_KIB, "内存不得低于兜底下限");
        assert!(p.mem_kib <= MEM_CEIL_KIB, "内存不得超过上限");
        assert!(p.iters >= ITERS_FLOOR);
        assert!(p.parallelism >= 1);
        // 标定出的参数必须可用于真实派生。
        let salt = [0u8; 16];
        assert!(derive_kek(b"x", &salt, &p).is_ok());
    }
}
