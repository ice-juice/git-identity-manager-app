//! Android / iOS 实现：两个操作在移动端都没有对应物。
//!
//! - `secure_key_file`：应用沙箱目录本身就只有本应用可读，不需要再收紧 ACL / chmod。
//! - `ssh_dir`：手机上没有会去读 `~/.ssh` 的 OpenSSH 消费者。返回沙箱内的占位目录，
//!   仅用于让调用方的路径拼接不 panic；真正依赖它的 `ssh::managed` / `agent`
//!   在移动端都不编译。

use super::PlatformOps;
use crate::error::Result;
use std::path::{Path, PathBuf};

pub struct Mobile;

impl PlatformOps for Mobile {
    fn secure_key_file(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn ssh_dir(&self) -> PathBuf {
        crate::identity::app_config_dir().join("ssh")
    }
}
