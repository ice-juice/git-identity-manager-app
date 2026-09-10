//! 非 Windows 平台能力降级标注。
//!
//! macOS / Linux 没有与 Windows 对等的剪贴板排除注册格式，
//! 清单必须如实标成 info，不得假装排除已生效。

pub fn clipboard_exclude_detail() -> String {
    "当前系统没有与 Windows 对等的剪贴板排除标记。复制机密只会走普通写入，系统剪贴板历史与第三方监听者仍可能记录。".into()
}

pub fn clipboard_exclude_advice() -> String {
    "复制后请尽快粘贴，并打开限时清空。不要把「排除标记」理解成防木马。".into()
}
