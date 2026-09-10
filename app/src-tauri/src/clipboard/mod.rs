//! 剪贴板写入：机密路径在 Windows 同一会话内打排除标记，避免进历史 / 云剪贴板。
//!
//! 非机密与 macOS/Linux 降级为 `arboard`。`clear_if_ours` 只清我们写入的那一份。

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use sha2::{Digest, Sha256};
#[cfg(any(test, not(windows)))]
use zeroize::Zeroize;

#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
mod unix;

/// 请求剪贴板监听者不要处理本次内容。
pub const FMT_EXCLUDE_MONITOR: &str = "ExcludeClipboardContentFromMonitorProcessing";
/// `DWORD 0`：不进 Win+V 剪贴板历史。
pub const FMT_CLIPBOARD_HISTORY: &str = "CanIncludeInClipboardHistory";
/// `DWORD 0`：不同步到云剪贴板。
pub const FMT_CLOUD_CLIPBOARD: &str = "CanUploadToCloudClipboard";

pub const OPEN_RETRIES: u32 = 5;
pub const OPEN_RETRY_MS: u64 = 20;

static HAS_WRITE: AtomicBool = AtomicBool::new(false);
static LAST_SEQ: AtomicU32 = AtomicU32::new(0);
static LAST_HASH: Mutex<Option<[u8; 32]>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOutcome {
    /// 三个排除格式已写入同一会话。
    pub excluded: bool,
    /// 机密路径因 `OpenClipboard` 占用而回退到普通写入。
    pub fallback: bool,
}

pub fn exclusion_supported() -> bool {
    #[cfg(windows)]
    {
        windows::exclusion_supported()
    }
    #[cfg(not(windows))]
    {
        unix::exclusion_supported()
    }
}

pub fn write(text: &str, secret: bool) -> Result<WriteOutcome, String> {
    #[cfg(windows)]
    if secret {
        match windows::write_excluded(text) {
            Ok(seq) => {
                remember_write(text, Some(seq));
                return Ok(WriteOutcome {
                    excluded: true,
                    fallback: false,
                });
            }
            Err(_) => {
                write_plain(text)?;
                return Ok(WriteOutcome {
                    excluded: false,
                    fallback: true,
                });
            }
        }
    }

    write_plain(text)?;
    let _ = secret;
    Ok(WriteOutcome {
        excluded: false,
        fallback: false,
    })
}

pub fn clear_if_ours() -> Result<(), String> {
    if !is_still_ours()? {
        forget_write();
        return Ok(());
    }
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.clear())
        .map_err(|e| e.to_string())?;
    forget_write();
    Ok(())
}

fn write_plain(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_text(text.to_string()))
        .map_err(|e| e.to_string())?;
    remember_write(text, current_seq());
    Ok(())
}

fn remember_write(text: &str, seq: Option<u32>) {
    if let Some(seq) = seq {
        LAST_SEQ.store(seq, Ordering::SeqCst);
        HAS_WRITE.store(true, Ordering::SeqCst);
    } else {
        HAS_WRITE.store(false, Ordering::SeqCst);
        LAST_SEQ.store(0, Ordering::SeqCst);
    }
    *LAST_HASH.lock().expect("clipboard hash lock") = Some(content_hash(text));
}

fn forget_write() {
    HAS_WRITE.store(false, Ordering::SeqCst);
    LAST_SEQ.store(0, Ordering::SeqCst);
    *LAST_HASH.lock().expect("clipboard hash lock") = None;
}

fn current_seq() -> Option<u32> {
    #[cfg(windows)]
    {
        Some(windows::sequence_number())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn is_still_ours() -> Result<bool, String> {
    #[cfg(windows)]
    {
        if !HAS_WRITE.load(Ordering::SeqCst) {
            return Ok(false);
        }
        Ok(should_clear_by_seq(
            Some(LAST_SEQ.load(Ordering::SeqCst)),
            windows::sequence_number(),
        ))
    }
    #[cfg(not(windows))]
    {
        let expected = *LAST_HASH.lock().expect("clipboard hash lock");
        let Some(expected) = expected else {
            return Ok(false);
        };
        let mut current = match arboard::Clipboard::new().and_then(|mut cb| cb.get_text()) {
            Ok(t) => t,
            Err(_) => return Ok(false),
        };
        let matched = should_clear_by_hash(Some(expected), content_hash(&current));
        current.zeroize();
        Ok(matched)
    }
}

pub(crate) fn encode_utf16_nul(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) fn dword_zero_bytes() -> [u8; 4] {
    0u32.to_le_bytes()
}

pub(crate) fn content_hash(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

/// 序列号仍是我们写入后记下的那一个，才允许清空。
pub(crate) fn should_clear_by_seq(last: Option<u32>, current: u32) -> bool {
    matches!(last, Some(seq) if seq == current)
}

#[cfg(any(test, not(windows)))]
pub(crate) fn should_clear_by_hash(last: Option<[u8; 32]>, current: [u8; 32]) -> bool {
    last == Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusion_format_names_match_spec() {
        assert_eq!(
            FMT_EXCLUDE_MONITOR,
            "ExcludeClipboardContentFromMonitorProcessing"
        );
        assert_eq!(FMT_CLIPBOARD_HISTORY, "CanIncludeInClipboardHistory");
        assert_eq!(FMT_CLOUD_CLIPBOARD, "CanUploadToCloudClipboard");
    }

    #[test]
    fn utf16_encoding_is_nul_terminated() {
        let w = encode_utf16_nul("密钥");
        assert_eq!(*w.last().unwrap(), 0);
        let decoded = String::from_utf16(&w[..w.len() - 1]).unwrap();
        assert_eq!(decoded, "密钥");
    }

    #[test]
    fn history_flags_are_le_dword_zero() {
        assert_eq!(dword_zero_bytes(), [0, 0, 0, 0]);
    }

    #[test]
    fn open_retry_policy_is_short() {
        assert_eq!(OPEN_RETRIES, 5);
        assert_eq!(OPEN_RETRY_MS, 20);
        assert!(u64::from(OPEN_RETRIES) * OPEN_RETRY_MS <= 100);
    }

    #[test]
    fn clear_only_when_sequence_still_ours() {
        assert!(should_clear_by_seq(Some(7), 7));
        assert!(!should_clear_by_seq(Some(7), 8));
        assert!(!should_clear_by_seq(None, 7));
    }

    #[test]
    fn clear_only_when_unix_hash_still_ours() {
        let a = content_hash("totp-123456");
        let b = content_hash("user-copied");
        assert!(should_clear_by_hash(Some(a), a));
        assert!(!should_clear_by_hash(Some(a), b));
        assert!(!should_clear_by_hash(None, a));
    }

    #[test]
    fn non_windows_does_not_claim_exclusion() {
        #[cfg(not(windows))]
        assert!(!exclusion_supported());
        #[cfg(windows)]
        assert!(exclusion_supported());
    }

    #[cfg(windows)]
    static CLIP_TEST: Mutex<()> = Mutex::new(());

    #[cfg(windows)]
    fn unique_token(tag: &str) -> String {
        format!("gam-clip-{tag}-{}", uuid::Uuid::new_v4())
    }

    #[cfg(windows)]
    #[test]
    fn secret_write_sets_exclusion_formats_and_clear_is_gated() {
        let _g = CLIP_TEST.lock().expect("clip test lock");
        let token = unique_token("secret");
        let outcome = write(&token, true).expect("secret write");
        if outcome.fallback {
            // 会话被占时必须回退而不是 panic；无法再断言格式。
            return;
        }
        assert!(outcome.excluded);
        assert!(windows::format_available(FMT_EXCLUDE_MONITOR));
        assert!(windows::format_available(FMT_CLIPBOARD_HISTORY));
        assert!(windows::format_available(FMT_CLOUD_CLIPBOARD));

        let mut got = arboard::Clipboard::new()
            .and_then(|mut cb| cb.get_text())
            .expect("read secret");
        assert_eq!(got, token);
        got.zeroize();

        let other = unique_token("user");
        arboard::Clipboard::new()
            .and_then(|mut cb| cb.set_text(other.clone()))
            .expect("user overwrite");
        clear_if_ours().expect("must not clear others");
        let after = arboard::Clipboard::new()
            .and_then(|mut cb| cb.get_text())
            .expect("read after skipped clear");
        assert_eq!(after, other);

        write(&token, true).ok();
        if HAS_WRITE.load(Ordering::SeqCst) {
            clear_if_ours().expect("clear ours");
        }
        let _ = arboard::Clipboard::new().and_then(|mut cb| cb.clear());
    }

    #[cfg(windows)]
    #[test]
    fn plain_write_does_not_set_exclusion_formats() {
        let _g = CLIP_TEST.lock().expect("clip test lock");
        let token = unique_token("plain");
        let outcome = write(&token, false).expect("plain write");
        assert!(!outcome.excluded);
        assert!(!outcome.fallback);
        assert!(!windows::format_available(FMT_EXCLUDE_MONITOR));
        assert!(!windows::format_available(FMT_CLIPBOARD_HISTORY));
        assert!(!windows::format_available(FMT_CLOUD_CLIPBOARD));
        let _ = token;
        forget_write();
        let _ = arboard::Clipboard::new().and_then(|mut cb| cb.clear());
    }
}
