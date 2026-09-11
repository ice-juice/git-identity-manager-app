//! Windows 原生剪贴板：同一会话写入 CF_UNICODETEXT + 三个排除格式。

use super::{
    dword_zero_bytes, encode_utf16_nul, FMT_CLIPBOARD_HISTORY, FMT_CLOUD_CLIPBOARD,
    FMT_EXCLUDE_MONITOR, OPEN_RETRIES, OPEN_RETRY_MS,
};
use std::time::Duration;
use windows_sys::Win32::Foundation::{GetLastError, GlobalFree, HANDLE, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
#[cfg(test)]
use windows_sys::Win32::System::DataExchange::IsClipboardFormatAvailable;
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use zeroize::Zeroize;

const CF_UNICODETEXT: u32 = 13;

pub fn exclusion_supported() -> bool {
    true
}

pub fn sequence_number() -> u32 {
    // SAFETY: 无参数查询，不打开剪贴板。
    unsafe { GetClipboardSequenceNumber() }
}

#[cfg(test)]
pub fn format_available(name: &str) -> bool {
    let fmt = match register_format(name) {
        Some(f) => f,
        None => return false,
    };
    // SAFETY: 已注册格式 ID，查询可用性不必打开剪贴板。
    unsafe { IsClipboardFormatAvailable(fmt) != 0 }
}

pub fn write_excluded(text: &str) -> Result<u32, String> {
    if !open_with_retry() {
        return Err("剪贴板被其他程序占用".into());
    }
    let result = write_while_open(text);
    // SAFETY: 与上方 OpenClipboard 成对。
    unsafe {
        CloseClipboard();
    }
    result?;
    Ok(sequence_number())
}

fn open_with_retry() -> bool {
    for attempt in 0..OPEN_RETRIES {
        // SAFETY: hwnd=NULL 将剪贴板关联到当前任务。
        let ok = unsafe { OpenClipboard(std::ptr::null_mut::<core::ffi::c_void>() as HWND) };
        if ok != 0 {
            return true;
        }
        if attempt + 1 < OPEN_RETRIES {
            std::thread::sleep(Duration::from_millis(OPEN_RETRY_MS));
        }
    }
    false
}

fn write_while_open(text: &str) -> Result<(), String> {
    // SAFETY: 已持有剪贴板。
    if unsafe { EmptyClipboard() } == 0 {
        return Err(last_err("EmptyClipboard"));
    }

    let mut wide = encode_utf16_nul(text);
    let bytes = wide.len().saturating_mul(2);
    let htext = match alloc_copy(wide.as_ptr() as *const u8, bytes) {
        Ok(h) => h,
        Err(e) => {
            wide.zeroize();
            return Err(e);
        }
    };
    wide.zeroize();

    // SAFETY: htext 为 GMEM_MOVEABLE；成功后所有权交给系统，不得 GlobalFree。
    let set = unsafe { SetClipboardData(CF_UNICODETEXT, htext) };
    if set.is_null() {
        unsafe {
            GlobalFree(htext);
        }
        return Err(last_err("SetClipboardData(CF_UNICODETEXT)"));
    }

    set_exclusion_formats();
    Ok(())
}

fn set_exclusion_formats() {
    if let Some(fmt) = register_format(FMT_EXCLUDE_MONITOR) {
        let _ = set_bytes(fmt, &[0u8]);
    }
    let zero = dword_zero_bytes();
    if let Some(fmt) = register_format(FMT_CLIPBOARD_HISTORY) {
        let _ = set_bytes(fmt, &zero);
    }
    if let Some(fmt) = register_format(FMT_CLOUD_CLIPBOARD) {
        let _ = set_bytes(fmt, &zero);
    }
}

fn register_format(name: &str) -> Option<u32> {
    let mut wide = encode_utf16_nul(name);
    // SAFETY: wide 以 NUL 结尾，API 只读该缓冲。
    let id = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
    wide.zeroize();
    if id == 0 {
        None
    } else {
        Some(id)
    }
}

fn set_bytes(format: u32, data: &[u8]) -> Result<(), String> {
    let handle = alloc_copy(data.as_ptr(), data.len())?;
    // SAFETY: 已持有剪贴板；成功后不得释放 handle。
    let set = unsafe { SetClipboardData(format, handle) };
    if set.is_null() {
        unsafe {
            GlobalFree(handle);
        }
        return Err(last_err("SetClipboardData(exclusion)"));
    }
    Ok(())
}

fn alloc_copy(src: *const u8, len: usize) -> Result<HANDLE, String> {
    if src.is_null() || len == 0 {
        return Err("GlobalAlloc 数据为空".into());
    }
    // SAFETY: 按 Win32 规则分配可移动全局内存。
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, len) };
    if handle.is_null() {
        return Err(last_err("GlobalAlloc"));
    }
    let ptr = unsafe { GlobalLock(handle) };
    if ptr.is_null() {
        unsafe {
            GlobalFree(handle);
        }
        return Err(last_err("GlobalLock"));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(src, ptr as *mut u8, len);
        GlobalUnlock(handle);
    }
    Ok(handle)
}

fn last_err(op: &str) -> String {
    // SAFETY: 读取本线程最近一次 Win32 错误。
    let code = unsafe { GetLastError() };
    format!("{op} 失败（Win32 {code}）")
}
