//! 云端同步与离线备份模块（M6 + v1.1）
//!
//! - `backup`: M6 本地加密备份导出与导入（.gambackup 格式）
//! - `s3`: S3 / Cloudflare R2 轻量存储客户端（SigV4 原生实现）
//! - `engine`: 端到端加密同步引擎（对象 HMAC 散列化、清单一致性保证）

pub mod backup;
pub mod engine;
pub mod s3;
pub mod scheduler;
