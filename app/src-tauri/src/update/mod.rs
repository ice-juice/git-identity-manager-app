//! 应用自更新：源解析、检查、启动静默检查。

pub mod checker;
pub mod scheduler;
pub mod source;

pub use crate::app_config::{UpdateSource, DEFAULT_UPDATE_REPO};
pub use source::{effective_source, resolve_endpoints};
