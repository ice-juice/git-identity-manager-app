//! macOS：登录钥匙串存随机 KEK，每次用 LocalAuthentication 弹 Touch ID / 面容。
//!
//! 不用 Keychain ACL / Data Protection 钥匙串：那条路径会写 `kSecAttrSynchronizable`
//! 或 `kSecUseDataProtectionKeychain`，未带 App ID entitlement 的签名（GitHub
//! 分发、ad-hoc、`tauri dev`）会得到 `A required entitlement isn't present`。

use crate::error::{AppError, Result};
use block2::RcBlock;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LABiometryType, LAContext, LAError, LAPolicy};
use security_framework::passwords::{
    delete_generic_password, generic_password, set_generic_password, PasswordOptions,
};
use std::sync::mpsc;
use std::time::Duration;

use super::BiometricAvailability;

const SERVICE: &str = "com.jeck.gitkeymaster.biometric";
const AUTH_TIMEOUT: Duration = Duration::from_secs(120);
const POLICY: LAPolicy = LAPolicy::DeviceOwnerAuthenticationWithBiometrics;

pub fn availability() -> BiometricAvailability {
    let ctx = unsafe { LAContext::new() };
    let available = unsafe { ctx.canEvaluatePolicy_error(POLICY) }.is_ok();
    let kind = match unsafe { ctx.biometryType() } {
        LABiometryType::FaceID => "face-id",
        _ => "touch-id",
    };
    BiometricAvailability {
        available,
        kind,
        strong: available,
    }
}

pub fn enroll(key_ref: &str, _challenge: &[u8]) -> Result<Vec<u8>> {
    if !availability().available {
        return Err(AppError::Invalid("本机未检测到可用的指纹 / 面容 ID".into()));
    }
    let domain = evaluate_biometrics("开启指纹解锁")?;
    let mut raw = vec![0u8; crate::vault::crypto::KEY_LEN];
    crate::vault::crypto::fill_random(&mut raw);
    delete_item(key_ref);
    delete_item(&domain_account(key_ref));
    set_generic_password(SERVICE, key_ref, &raw)
        .map_err(|e| AppError::Other(format!("无法写入钥匙串：{e}")))?;
    if !domain.is_empty() {
        let _ = set_generic_password(SERVICE, &domain_account(key_ref), &domain);
    }
    Ok(raw)
}

pub fn derive(key_ref: &str, _challenge: &[u8]) -> Result<Vec<u8>> {
    let domain = evaluate_biometrics("解锁工作空间")?;
    ensure_domain_current(key_ref, &domain)?;
    generic_password(PasswordOptions::new_generic_password(SERVICE, key_ref)).map_err(map_keychain)
}

pub fn verify_presence(prompt: &str) -> Result<()> {
    let file = super::store::load().map_err(|_| AppError::BiometricStale)?;
    let domain = evaluate_biometrics(prompt)?;
    ensure_domain_current(&file.key_ref, &domain)
}

pub fn remove(key_ref: &str) -> Result<()> {
    delete_item(key_ref);
    delete_item(&domain_account(key_ref));
    Ok(())
}

fn domain_account(key_ref: &str) -> String {
    format!("{key_ref}.domain")
}

fn delete_item(account: &str) {
    if let Err(e) = delete_generic_password(SERVICE, account) {
        let msg = e.to_string();
        if !(msg.contains("-25300") || msg.to_lowercase().contains("not found")) {
            log::debug!("删除钥匙串项 {account} 失败：{e}");
        }
    }
}

fn ensure_domain_current(key_ref: &str, current: &[u8]) -> Result<()> {
    let stored = match generic_password(PasswordOptions::new_generic_password(
        SERVICE,
        &domain_account(key_ref),
    )) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    if stored.is_empty() || current.is_empty() || stored == current {
        Ok(())
    } else {
        Err(AppError::BiometricStale)
    }
}

fn evaluate_biometrics(reason: &str) -> Result<Vec<u8>> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(AppError::Invalid("指纹验证原因不能为空".into()));
    }
    let context = unsafe { LAContext::new() };
    if let Err(err) = unsafe { context.canEvaluatePolicy_error(POLICY) } {
        return Err(map_la_error(&err));
    }
    unsafe {
        context.setLocalizedFallbackTitle(Some(&NSString::from_str("")));
    }

    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |success, error: *mut NSError| {
        let outcome = if bool::from(success) {
            Ok(())
        } else if error.is_null() {
            Err(AppError::Other("指纹验证失败".into()))
        } else {
            Err(map_la_error(unsafe { &*error }))
        };
        let _ = tx.send(outcome);
    });
    unsafe {
        context.evaluatePolicy_localizedReason_reply(POLICY, &NSString::from_str(reason), &block);
    }
    match rx.recv_timeout(AUTH_TIMEOUT) {
        Ok(Ok(())) => Ok(read_domain_state(&context)),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(AppError::Other("指纹验证超时".into())),
    }
}

fn read_domain_state(ctx: &LAContext) -> Vec<u8> {
    #[allow(deprecated)]
    unsafe { ctx.evaluatedPolicyDomainState() }
        .map(|data| data.to_vec())
        .unwrap_or_default()
}

fn map_la_error(err: &NSError) -> AppError {
    match LAError(err.code()) {
        LAError::UserCancel
        | LAError::SystemCancel
        | LAError::AppCancel
        | LAError::UserFallback => AppError::BiometricCancelled,
        LAError::BiometryNotAvailable | LAError::BiometryNotEnrolled => {
            AppError::Invalid("本机未检测到可用的指纹 / 面容 ID".into())
        }
        LAError::BiometryLockout => {
            AppError::Invalid("指纹已锁定，请先在系统中解锁，或改用访问密码".into())
        }
        LAError::PasscodeNotSet => {
            AppError::Invalid("请先在系统中设置开机密码后再开启指纹解锁".into())
        }
        other => AppError::Other(format!("指纹验证失败（{}）", other.0)),
    }
}

fn map_keychain(e: security_framework::base::Error) -> AppError {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    if msg.contains("-128") || lower.contains("cancel") {
        AppError::BiometricCancelled
    } else if msg.contains("-25300") || lower.contains("not found") {
        AppError::BiometricStale
    } else {
        AppError::Other(format!("无法读取钥匙串：{e}"))
    }
}
