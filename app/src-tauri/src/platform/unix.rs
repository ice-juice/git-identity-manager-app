//! 类 Unix 平台实现（预留，便于日后 macOS/Linux）。

use super::PlatformOps;
use crate::error::Result;
use crate::sys;
use std::path::{Path, PathBuf};

pub struct Unix;

impl PlatformOps for Unix {
    fn secure_key_file(&self, path: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perm = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path, perm)?;
        }
        let _ = path;
        Ok(())
    }
    fn ssh_dir(&self) -> PathBuf {
        sys::ssh_dir()
    }
}
