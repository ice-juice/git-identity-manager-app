//! macOS / Linux：无系统级剪贴板排除格式，机密写入降级为普通 arboard 路径。

pub fn exclusion_supported() -> bool {
    false
}
