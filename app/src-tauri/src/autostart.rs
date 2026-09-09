//! 开机自启动（当前用户 Run 项）。

use crate::error::{AppError, Result};

const VALUE_NAME: &str = "GitAccountManager";

#[cfg(windows)]
pub fn set_enabled(enabled: bool) -> Result<()> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            KEY_SET_VALUE,
        )
        .or_else(|_| {
            hkcu.create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
                .map(|(k, _)| k)
        })
        .map_err(|e| AppError::Io(format!("无法写入开机启动项：{e}")))?;
    if enabled {
        let exe = std::env::current_exe().map_err(|e| AppError::Io(e.to_string()))?;
        let path = format!("\"{}\"", exe.display());
        key.set_value(VALUE_NAME, &path)
            .map_err(|e| AppError::Io(format!("写入开机启动项失败：{e}")))?;
    } else {
        let _ = key.delete_value(VALUE_NAME);
    }
    Ok(())
}

#[cfg(windows)]
pub fn is_enabled() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run") else {
        return false;
    };
    key.get_value::<String, _>(VALUE_NAME).is_ok()
}

#[cfg(not(windows))]
pub fn set_enabled(_enabled: bool) -> Result<()> {
    Err(AppError::Invalid("开机自启动仅支持 Windows".into()))
}

#[cfg(not(windows))]
pub fn is_enabled() -> bool {
    false
}
