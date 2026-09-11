//! 本机网络代理：拼 URL、挂到 reqwest / updater / git / ssh。

pub mod proxy;

pub use proxy::{
    apply_git_command, apply_reqwest_async, apply_reqwest_blocking, apply_ssh_command, apply_updater,
    effective, for_cloud_sync, proxy_url, ssh_auth_unsupported, ssh_helper_status, test_github_https,
    SshProxyHelper,
};
