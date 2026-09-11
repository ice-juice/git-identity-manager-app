//! 开机 / 登录自启动。Windows 写 HKCU Run；macOS 写用户 LaunchAgent。

use crate::error::{AppError, Result};

#[cfg(windows)]
const VALUE_NAME: &str = "GitAccountManager";
#[cfg(target_os = "macos")]
const LAUNCH_AGENT_LABEL: &str = "com.jeck.gitkeymaster";
#[cfg(target_os = "macos")]
const LAUNCH_AGENT_FILE: &str = "com.jeck.gitkeymaster.plist";

pub fn is_supported() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

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

#[cfg(target_os = "macos")]
pub fn set_enabled(enabled: bool) -> Result<()> {
    let path = launch_agent_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if enabled {
        let plist = launch_agent_plist()?;
        std::fs::write(&path, plist)?;
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&path)
            .status();
        let status = std::process::Command::new("launchctl")
            .args(["load", "-w"])
            .arg(&path)
            .status()
            .map_err(|e| AppError::Io(format!("无法加载登录启动项：{e}")))?;
        if !status.success() {
            return Err(AppError::Io(
                "无法加载登录启动项。若系统提示需要允许后台运行，请在「登录项与扩展」中打开御钥师。"
                    .into(),
            ));
        }
    } else {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&path)
            .status();
        let _ = std::fs::remove_file(&path);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn is_enabled() -> bool {
    launch_agent_path().map(|p| p.is_file()).unwrap_or(false)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn set_enabled(_enabled: bool) -> Result<()> {
    Err(AppError::Invalid("当前系统暂不支持开机自启动".into()))
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn is_enabled() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn launch_agent_path() -> Result<std::path::PathBuf> {
    let home = std::env::var("HOME").map_err(|_| AppError::Io("找不到用户主目录".into()))?;
    Ok(std::path::PathBuf::from(home)
        .join("Library/LaunchAgents")
        .join(LAUNCH_AGENT_FILE))
}

#[cfg(target_os = "macos")]
fn launch_agent_plist() -> Result<String> {
    let exe = std::env::current_exe().map_err(|e| AppError::Io(e.to_string()))?;
    let (program, args) = match app_bundle_path(&exe) {
        Some(app) => (
            "/usr/bin/open".to_string(),
            vec!["-a".to_string(), app.to_string_lossy().into_owned()],
        ),
        None => (exe.to_string_lossy().into_owned(), Vec::new()),
    };
    let mut arguments = format!("      <string>{}</string>\n", xml_escape(&program));
    for arg in args {
        arguments.push_str(&format!("      <string>{}</string>\n", xml_escape(&arg)));
    }
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
{arguments}  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>LimitLoadToSessionType</key>
  <string>Aqua</string>
</dict>
</plist>
"#,
        label = LAUNCH_AGENT_LABEL,
        arguments = arguments,
    ))
}

#[cfg(target_os = "macos")]
fn app_bundle_path(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    if macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension().is_some_and(|ext| ext == "app")
    {
        Some(app.to_path_buf())
    } else {
        None
    }
}

#[cfg(any(target_os = "macos", test))]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::xml_escape;

    #[test]
    fn xml_escape_ampersand() {
        assert_eq!(xml_escape(r#"C:\a&b<c>"#), r#"C:\a&amp;b&lt;c&gt;"#);
    }
}
