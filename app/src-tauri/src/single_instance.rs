//! 单实例检测：同一主机（同一用户会话）不允许同时运行两个本程序，
//! 以免多个实例并发写入工作空间 / SSH 配置 / ssh-agent 造成冲突。
//!
//! 机制：在临时目录放一个以应用标识命名的锁文件，内容为
//! “PID + 可执行文件路径 + 本机回环端口 + 一次性口令”。
//! 持锁实例在 `127.0.0.1` 上监听退出通道；新实例若发现冲突，
//! 弹出原生对话框，经用户确认后向该通道发送退出请求。
//! 旧实例自行锁定保险库再退出，新实例等其进程结束后再接管。
//! 崩溃残留的锁会因 PID 不再存活而被自动忽略，无需额外清理即可自愈。

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use std::{fs, thread};

/// 启动决策。
pub enum Decision {
    /// 无冲突，或旧实例已优雅退出——可继续启动。
    Proceed,
    /// 用户选择取消，或无法通知旧实例退出——调用方应立即退出进程。
    Exit,
}

fn lock_path() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push("com.jeck.gitaccountmanager.instance.lock");
    p
}

fn current_exe_string() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_default()
}

struct LockInfo {
    pid: u32,
    exe: String,
    port: Option<u16>,
    token: Option<String>,
}

fn read_lock() -> Option<LockInfo> {
    let content = fs::read_to_string(lock_path()).ok()?;
    let mut lines = content.lines();
    let pid: u32 = lines.next()?.trim().parse().ok()?;
    let exe = lines.next().unwrap_or("").trim().to_string();
    let port = lines.next().and_then(|s| s.trim().parse().ok());
    let token = lines
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Some(LockInfo {
        pid,
        exe,
        port,
        token,
    })
}

fn random_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static QUIT_HANDLER: Mutex<Option<Box<dyn FnOnce() + Send>>> = Mutex::new(None);

fn fire_quit() {
    QUIT_REQUESTED.store(true, Ordering::SeqCst);
    let handler = QUIT_HANDLER.lock().ok().and_then(|mut slot| slot.take());
    if let Some(handler) = handler {
        handler();
    }
}

/// Tauri 就绪后注册真正的退出动作（锁定保险库并退出事件循环）。
/// 若通道请求已先到达，会立即执行该动作。
pub fn on_ready<F>(handler: F)
where
    F: FnOnce() + Send + 'static,
{
    if QUIT_REQUESTED.load(Ordering::SeqCst) {
        handler();
        return;
    }
    if let Ok(mut slot) = QUIT_HANDLER.lock() {
        *slot = Some(Box::new(handler));
    }
    if QUIT_REQUESTED.load(Ordering::SeqCst) {
        let handler = QUIT_HANDLER.lock().ok().and_then(|mut slot| slot.take());
        if let Some(handler) = handler {
            handler();
        }
    }
}

fn serve_quit(listener: TcpListener, token: String) {
    let _ = listener.set_nonblocking(false);
    loop {
        let Ok((mut stream, addr)) = listener.accept() else {
            continue;
        };
        if !addr.ip().is_loopback() {
            continue;
        }
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
        let mut buf = [0u8; 128];
        let Ok(n) = stream.read(&mut buf) else {
            continue;
        };
        let req = String::from_utf8_lossy(&buf[..n]);
        let expected = format!("QUIT {token}");
        if req.trim() != expected {
            continue;
        }
        let _ = stream.write_all(b"OK\n");
        let _ = stream.flush();
        fire_quit();
        break;
    }
}

fn write_lock() {
    let token = random_token();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok();
    let port = listener
        .as_ref()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
        .unwrap_or(0);
    let content = format!(
        "{}\n{}\n{}\n{}\n",
        std::process::id(),
        current_exe_string(),
        port,
        token
    );
    let _ = fs::write(lock_path(), content);
    if let Some(listener) = listener {
        let _ = thread::Builder::new()
            .name("instance-quit".into())
            .spawn(move || serve_quit(listener, token));
    }
}

/// 进程退出时释放锁（仅当锁仍归属于本进程时才删除，避免误删接管者的锁）。
pub fn release_lock() {
    if let Some(info) = read_lock() {
        if info.pid == std::process::id() {
            let _ = fs::remove_file(lock_path());
        }
    }
}

fn wait_until_gone(pid: u32, exe: &str, timeout: Duration) -> bool {
    let steps = (timeout.as_millis() / 100).max(1) as u32;
    for _ in 0..steps {
        if !imp::is_running(pid, exe) {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    !imp::is_running(pid, exe)
}

fn try_send_quit(port: u16, token: &str) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    let msg = format!("QUIT {token}\n");
    if stream.write_all(msg.as_bytes()).is_err() {
        return false;
    }
    let _ = stream.flush();
    let mut buf = [0u8; 16];
    let Ok(n) = stream.read(&mut buf) else {
        return false;
    };
    String::from_utf8_lossy(&buf[..n]).trim() == "OK"
}

fn request_graceful_quit(info: &LockInfo) -> bool {
    let Some(port) = info.port.filter(|p| *p > 0) else {
        return false;
    };
    let Some(token) = info.token.as_deref() else {
        return false;
    };
    for _ in 0..15 {
        if try_send_quit(port, token) {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

/// 启动检查：检测已运行实例并在冲突时弹窗询问。
pub fn check() -> Decision {
    if let Some(info) = read_lock() {
        let me = std::process::id();
        if info.pid != me && imp::is_running(info.pid, &info.exe) {
            if !imp::prompt_close_existing() {
                return Decision::Exit;
            }
            if !imp::is_running(info.pid, &info.exe) {
                write_lock();
                return Decision::Proceed;
            }
            if info.port.filter(|p| *p > 0).is_none() || info.token.is_none() {
                imp::prompt_close_failed();
                return Decision::Exit;
            }
            let _ = request_graceful_quit(&info);
            if wait_until_gone(info.pid, &info.exe, Duration::from_secs(10)) {
                write_lock();
                return Decision::Proceed;
            }
            imp::prompt_close_failed();
            return Decision::Exit;
        }
    }
    write_lock();
    Decision::Proceed
}

const DIALOG_TITLE: &str = "御钥师 已在运行";
const DIALOG_BODY: &str = "检测到本机已经有一个「御钥师」正在运行。\n\n为避免多个实例同时写入工作空间、SSH 配置与 ssh-agent 造成冲突，同一时间只能运行一个。\n\n是否关闭已运行的那个（它会先锁定保险库再退出），并继续启动当前程序？";
const FAIL_TITLE: &str = "无法关闭已运行的实例";
const FAIL_BODY: &str = "已运行的「御钥师」没有响应关闭请求。请先手动退出那个程序，再重新打开。";

// ===================== Windows 实现 =====================
#[cfg(windows)]
mod imp {
    use super::{DIALOG_BODY, DIALOG_TITLE, FAIL_BODY, FAIL_TITLE};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 0x0000_0102;
    const STILL_RUNNING_ACCESS: u32 = PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE;

    const MB_OK: u32 = 0x0000_0000;
    const MB_YESNO: u32 = 0x0000_0004;
    const MB_ICONERROR: u32 = 0x0000_0010;
    const MB_ICONWARNING: u32 = 0x0000_0030;
    const MB_SETFOREGROUND: u32 = 0x0001_0000;
    const MB_TOPMOST: u32 = 0x0004_0000;
    const IDYES: i32 = 6;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn open(pid: u32) -> isize {
        // SAFETY: 调用 Win32 API，参数为普通标量。
        unsafe { OpenProcess(STILL_RUNNING_ACCESS, 0, pid) as isize }
    }

    fn image_path(handle: isize) -> Option<String> {
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        // SAFETY: buf/size 均为合法可写内存。
        let ok = unsafe { QueryFullProcessImageNameW(handle as _, 0, buf.as_mut_ptr(), &mut size) };
        if ok == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..size as usize]))
    }

    pub fn is_running(pid: u32, exe: &str) -> bool {
        if pid == 0 {
            return false;
        }
        let handle = open(pid);
        if handle == 0 {
            return false;
        }
        // SAFETY: handle 为有效句柄。
        let alive = unsafe { WaitForSingleObject(handle as _, 0) } == WAIT_TIMEOUT;
        let same_image = if exe.is_empty() {
            true
        } else {
            image_path(handle)
                .map(|p| p.eq_ignore_ascii_case(exe))
                .unwrap_or(true)
        };
        // SAFETY: 关闭前面打开的句柄。
        unsafe { CloseHandle(handle as _) };
        alive && same_image
    }

    fn message_box(title: &str, body: &str, flags: u32) -> i32 {
        let text = wide(body);
        let caption = wide(title);
        // SAFETY: 传入的宽字符串以 NUL 结尾且在调用期间有效。
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                flags,
            )
        }
    }

    pub fn prompt_close_existing() -> bool {
        message_box(
            DIALOG_TITLE,
            DIALOG_BODY,
            MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST,
        ) == IDYES
    }

    pub fn prompt_close_failed() {
        let _ = message_box(
            FAIL_TITLE,
            FAIL_BODY,
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST,
        );
    }
}

// ===================== 非 Windows 实现 =====================
#[cfg(not(windows))]
mod imp {
    use super::{DIALOG_BODY, DIALOG_TITLE, FAIL_BODY, FAIL_TITLE};

    pub fn is_running(pid: u32, _exe: &str) -> bool {
        if pid == 0 {
            return false;
        }
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn prompt_close_existing() -> bool {
        let res = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title(DIALOG_TITLE)
            .set_description(format!("{DIALOG_BODY}\n\n（是 = 关闭并继续；否 = 取消运行）"))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show();
        matches!(res, rfd::MessageDialogResult::Yes)
    }

    pub fn prompt_close_failed() {
        let _ = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title(FAIL_TITLE)
            .set_description(FAIL_BODY)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }
}
