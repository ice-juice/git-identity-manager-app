//! macOS：Keychain ACL 存随机 KEK（路线 B）+ LocalAuthentication（路线 A）。

use crate::error::{AppError, Result};
use security_framework::passwords::{
    delete_generic_password, generic_password, set_generic_password_options, AccessControlOptions,
    PasswordOptions,
};

use super::BiometricAvailability;

const SERVICE: &str = "com.jeck.gitkeymaster.biometric";

#[link(name = "LocalAuthentication", kind = "framework")]
extern "C" {}

pub fn availability() -> BiometricAvailability {
    BiometricAvailability {
        available: la_can_evaluate(),
        kind: "touch-id",
        strong: true,
    }
}

pub fn enroll(key_ref: &str, _challenge: &[u8]) -> Result<Vec<u8>> {
    let mut raw = vec![0u8; crate::vault::crypto::KEY_LEN];
    crate::vault::crypto::fill_random(&mut raw);
    let _ = delete_generic_password(SERVICE, key_ref);
    let mut opts = PasswordOptions::new_generic_password(SERVICE, key_ref);
    opts.set_access_control_options(AccessControlOptions::BIOMETRY_CURRENT_SET);
    opts.set_access_synchronized(Some(false));
    set_generic_password_options(&raw, opts)
        .map_err(|e| AppError::Other(format!("无法写入钥匙串：{e}")))?;
    Ok(raw)
}

pub fn derive(key_ref: &str, _challenge: &[u8]) -> Result<Vec<u8>> {
    generic_password(PasswordOptions::new_generic_password(SERVICE, key_ref)).map_err(map_keychain)
}

pub fn verify_presence(prompt: &str) -> Result<()> {
    la_evaluate(prompt)
}

pub fn remove(key_ref: &str) -> Result<()> {
    match delete_generic_password(SERVICE, key_ref) {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("-25300") || msg.to_lowercase().contains("not found") {
                Ok(())
            } else {
                Err(AppError::Other(format!("无法删除钥匙串项：{e}")))
            }
        }
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

fn la_can_evaluate() -> bool {
    use objc::runtime::{Object, BOOL, YES};
    use objc::{class, msg_send};
    unsafe {
        let ctx: *mut Object = msg_send![class!(LAContext), new];
        if ctx.is_null() {
            return false;
        }
        let mut err: *mut Object = std::ptr::null_mut();
        let ok: BOOL = msg_send![ctx, canEvaluatePolicy: 1u64 error: &mut err];
        let _: () = msg_send![ctx, release];
        ok == YES
    }
}

fn la_evaluate(prompt: &str) -> Result<()> {
    use block::ConcreteBlock;
    use objc::runtime::{Object, BOOL, YES};
    use objc::{class, msg_send};
    use std::sync::mpsc;
    use std::time::Duration;

    unsafe {
        let ctx: *mut Object = msg_send![class!(LAContext), new];
        if ctx.is_null() {
            return Err(AppError::Invalid("无法创建本地认证上下文".into()));
        }
        let reason = nsstring(prompt);
        let (tx, rx) = mpsc::channel::<(bool, i64)>();
        let reply = ConcreteBlock::new(move |success: BOOL, error: *mut Object| {
            let code = if error.is_null() {
                0
            } else {
                msg_send![error, code]
            };
            let _ = tx.send((success == YES, code));
        });
        let reply = reply.copy();
        let _: () = msg_send![
            ctx,
            evaluatePolicy: 1u64
            localizedReason: reason
            reply: &*reply
        ];
        let recv = rx.recv_timeout(Duration::from_secs(90));
        let _: () = msg_send![reason, release];
        let _: () = msg_send![ctx, release];
        match recv {
            Ok((true, _)) => Ok(()),
            Ok((false, -2 | -3 | -4 | -9)) => Err(AppError::BiometricCancelled),
            Ok((false, -6 | -7)) => Err(AppError::Invalid("本机未登记指纹或面容".into())),
            Ok((false, -8)) => Err(AppError::Invalid("生物识别已锁定，请改用密码".into())),
            Ok((false, _)) => Err(AppError::Invalid("生物识别未通过".into())),
            Err(_) => Err(AppError::Invalid("生物识别超时".into())),
        }
    }
}

fn nsstring(s: &str) -> *mut objc::runtime::Object {
    use objc::runtime::Object;
    use objc::{class, msg_send};
    let c = std::ffi::CString::new(s).unwrap_or_default();
    unsafe {
        let ns: *mut Object = msg_send![class!(NSString), alloc];
        msg_send![ns, initWithUTF8String: c.as_ptr()]
    }
}
