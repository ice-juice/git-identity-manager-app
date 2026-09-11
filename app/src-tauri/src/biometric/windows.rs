//! Windows Hello：KeyCredentialManager（路线 B）+ UserConsentVerifier（路线 A）。

use crate::error::{AppError, Result};
use ::windows::core::{factory, HSTRING};
use ::windows::Security::Credentials::UI::{
    UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
};
use ::windows::Security::Credentials::{
    KeyCredentialCreationOption, KeyCredentialManager, KeyCredentialStatus,
};
use ::windows::Security::Cryptography::CryptographicBuffer;
use ::windows::Storage::Streams::IBuffer;
use ::windows::Win32::Foundation::HWND;
use ::windows::Win32::System::WinRT::IUserConsentVerifierInterop;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::BiometricAvailability;

pub fn availability() -> BiometricAvailability {
    let available = match UserConsentVerifier::CheckAvailabilityAsync() {
        Ok(op) => match op.get() {
            Ok(UserConsentVerifierAvailability::Available) => true,
            _ => false,
        },
        Err(_) => false,
    };
    let strong = KeyCredentialManager::IsSupportedAsync()
        .and_then(|op| op.get())
        .unwrap_or(false);
    BiometricAvailability {
        available: available || strong,
        kind: "windows-hello",
        strong,
    }
}

pub fn enroll(key_ref: &str, challenge: &[u8]) -> Result<Vec<u8>> {
    with_hello_dialog_visible(|| enroll_inner(key_ref, challenge))
}

fn enroll_inner(key_ref: &str, challenge: &[u8]) -> Result<Vec<u8>> {
    let name = HSTRING::from(key_ref);
    let retrieval = KeyCredentialManager::RequestCreateAsync(
        &name,
        KeyCredentialCreationOption::ReplaceExisting,
    )
    .and_then(|op| op.get())
    .map_err(map_win)?;
    ensure_key_status(retrieval.Status().map_err(map_win)?)?;
    let cred = retrieval.Credential().map_err(map_win)?;
    let buf = to_buffer(challenge)?;
    let op = cred.RequestSignAsync(&buf).map_err(map_win)?;
    let signed = op.get().map_err(map_win)?;
    ensure_key_status(signed.Status().map_err(map_win)?)?;
    let sig = signed.Result().map_err(map_win)?;
    from_buffer(&sig)
}

pub fn derive(key_ref: &str, challenge: &[u8]) -> Result<Vec<u8>> {
    with_hello_dialog_visible(|| derive_inner(key_ref, challenge))
}

fn derive_inner(key_ref: &str, challenge: &[u8]) -> Result<Vec<u8>> {
    let name = HSTRING::from(key_ref);
    let retrieval = KeyCredentialManager::OpenAsync(&name)
        .and_then(|op| op.get())
        .map_err(map_win)?;
    ensure_key_status(retrieval.Status().map_err(map_win)?)?;
    let cred = retrieval.Credential().map_err(map_win)?;
    let buf = to_buffer(challenge)?;
    let signed = cred
        .RequestSignAsync(&buf)
        .and_then(|op| op.get())
        .map_err(map_win)?;
    ensure_key_status(signed.Status().map_err(map_win)?)?;
    let sig = signed.Result().map_err(map_win)?;
    from_buffer(&sig)
}

pub fn verify_presence(prompt: &str) -> Result<()> {
    with_hello_dialog_visible(|| verify_presence_inner(prompt))
}

fn verify_presence_inner(prompt: &str) -> Result<()> {
    let hwnd = resolve_hwnd();
    let interop: IUserConsentVerifierInterop =
        factory::<UserConsentVerifier, IUserConsentVerifierInterop>().map_err(map_win)?;
    let operation: windows_future::IAsyncOperation<UserConsentVerificationResult> = unsafe {
        interop.RequestVerificationForWindowAsync(
            HWND(hwnd as *mut core::ffi::c_void),
            &HSTRING::from(prompt),
        )
    }
    .map_err(map_win)?;
    let result = operation.get().map_err(map_win)?;
    match result {
        UserConsentVerificationResult::Verified => Ok(()),
        UserConsentVerificationResult::Canceled => Err(AppError::BiometricCancelled),
        UserConsentVerificationResult::RetriesExhausted => {
            Err(AppError::Invalid("生物识别重试次数已用尽".into()))
        }
        UserConsentVerificationResult::DeviceNotPresent => {
            Err(AppError::Invalid("未检测到 Windows Hello 设备".into()))
        }
        UserConsentVerificationResult::NotConfiguredForUser => {
            Err(AppError::Invalid("当前 Windows 用户未配置 Windows Hello".into()))
        }
        UserConsentVerificationResult::DisabledByPolicy => {
            Err(AppError::Invalid("组织策略已禁用 Windows Hello".into()))
        }
        other => Err(AppError::Invalid(format!("Windows Hello 验证未通过（{other:?}）"))),
    }
}

pub fn remove(key_ref: &str) -> Result<()> {
    let name = HSTRING::from(key_ref);
    match KeyCredentialManager::DeleteAsync(&name).and_then(|op| op.get()) {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = e.message();
            if msg.is_empty() {
                Ok(())
            } else {
                // 密钥本就不存在时视为已清理。
                Ok(())
            }
        }
    }
}

fn resolve_hwnd() -> isize {
    if let Some(h) = super::prompt_hwnd() {
        return h;
    }
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() as isize }
}

/// KeyCredential / Hello 系统窗无法画进 WebView，只能挂到主窗口并强制提到桌面前台，
/// 避免只在任务栏闪一个独立按钮。
fn with_hello_dialog_visible<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    let owner = resolve_hwnd();
    raise_owner_window(owner);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_watch = stop.clone();
    let watch = std::thread::spawn(move || {
        while !stop_watch.load(Ordering::Relaxed) {
            promote_hello_windows(owner);
            std::thread::sleep(Duration::from_millis(80));
        }
    });
    let result = f();
    stop.store(true, Ordering::Relaxed);
    let _ = watch.join();
    result
}

fn raise_owner_window(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            AllowSetForegroundWindow, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
        };
        let h = hwnd as windows_sys::Win32::Foundation::HWND;
        if IsIconic(h) != 0 {
            ShowWindow(h, SW_RESTORE);
        } else {
            ShowWindow(h, SW_SHOW);
        }
        AllowSetForegroundWindow(u32::MAX);
        SetForegroundWindow(h);
    }
}

fn promote_hello_windows(owner: isize) {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows(
            Some(enum_hello_windows),
            owner as windows_sys::Win32::Foundation::LPARAM,
        );
    }
}

unsafe extern "system" fn enum_hello_windows(
    hwnd: windows_sys::Win32::Foundation::HWND,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::BOOL {
    let owner = lparam as windows_sys::Win32::Foundation::HWND;
    if hwnd == owner || !is_hello_dialog(hwnd) {
        return 1;
    }
    raise_hello_dialog(hwnd, owner);
    1
}

fn is_hello_dialog(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    let class = native_window_text(hwnd, true).to_ascii_lowercase();
    let title = native_window_text(hwnd, false).to_ascii_lowercase();
    if class.contains("credential dialog") {
        return true;
    }
    if class.contains("xaml_windowedpopup")
        && (title.contains("hello") || title.contains("security") || title.contains("安全"))
    {
        return true;
    }
    title.contains("windows hello")
        || title.contains("windows security")
        || title.contains("windows 安全")
}

fn native_window_text(hwnd: windows_sys::Win32::Foundation::HWND, class_name: bool) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe {
        if class_name {
            windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW(
                hwnd,
                buf.as_mut_ptr(),
                buf.len() as i32,
            )
        } else {
            windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW(
                hwnd,
                buf.as_mut_ptr(),
                buf.len() as i32,
            )
        }
    };
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

fn raise_hello_dialog(
    hwnd: windows_sys::Win32::Foundation::HWND,
    owner: windows_sys::Win32::Foundation::HWND,
) {
    use windows_sys::Win32::System::Threading::AttachThreadInput;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
        SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_HWNDPARENT,
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE, SW_SHOW,
    };
    unsafe {
        if owner as isize != 0 {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner as isize);
        }
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        AllowSetForegroundWindow(u32::MAX);
        let fg = GetForegroundWindow();
        let mut fg_pid = 0u32;
        let fg_tid = GetWindowThreadProcessId(fg, &mut fg_pid);
        let mut hello_pid = 0u32;
        let hello_tid = GetWindowThreadProcessId(hwnd, &mut hello_pid);
        if fg_tid != 0 && hello_tid != 0 && fg_tid != hello_tid {
            let _ = AttachThreadInput(fg_tid, hello_tid, 1);
            SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(fg_tid, hello_tid, 0);
        } else {
            SetForegroundWindow(hwnd);
        }
        SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
    }
}

fn ensure_key_status(status: KeyCredentialStatus) -> Result<()> {
    match status {
        KeyCredentialStatus::Success => Ok(()),
        KeyCredentialStatus::UserCanceled => Err(AppError::BiometricCancelled),
        KeyCredentialStatus::NotFound => Err(AppError::BiometricStale),
        KeyCredentialStatus::SecurityDeviceLocked => {
            Err(AppError::Invalid("安全设备已锁定，请稍后再试".into()))
        }
        other => Err(AppError::Invalid(format!(
            "Windows Hello 密钥操作失败（{other:?}）"
        ))),
    }
}

fn to_buffer(bytes: &[u8]) -> Result<IBuffer> {
    CryptographicBuffer::CreateFromByteArray(bytes).map_err(map_win)
}

fn from_buffer(buf: &IBuffer) -> Result<Vec<u8>> {
    use windows::Storage::Streams::DataReader;
    let reader = DataReader::FromBuffer(buf).map_err(map_win)?;
    let len = buf.Length().map_err(map_win)? as usize;
    let mut out = vec![0u8; len];
    reader.ReadBytes(&mut out).map_err(map_win)?;
    Ok(out)
}

fn map_win(e: ::windows::core::Error) -> AppError {
    let msg = e.message();
    if msg.is_empty() {
        AppError::Other(format!("Windows Hello 调用失败：0x{:08X}", e.code().0 as u32))
    } else {
        AppError::Other(format!("Windows Hello：{msg}"))
    }
}
