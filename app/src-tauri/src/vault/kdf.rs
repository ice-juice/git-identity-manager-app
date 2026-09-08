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
pub fn calibrate(target_ms: u128) -> KdfParams {
    let par = parallelism();
    // 从 64MiB / 2 次起步（保证不低于兜底），先加内存再加迭代。
    let mut mem = 64u32 * 1024;
    let mut iters = ITERS_FLOOR;

    // 阶段一：抬内存到 512MiB 或首次达标。
    loop {
        let p = KdfParams::new(mem, iters, par);
        if measure_ms(&p) >= target_ms || mem >= MEM_CEIL_KIB {
            break;
        }
        mem = (mem * 2).min(MEM_CEIL_KIB);
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
