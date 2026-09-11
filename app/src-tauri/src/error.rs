//! 统一领域错误类型：面向用户的中文信息 + 机器可读 code。
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("工作空间已存在：{0}")]
    AlreadyInitialized(String),
    #[error("工作空间不存在或未初始化")]
    NotInitialized,
    #[error("访问密码错误，或数据已损坏")]
    BadPassword,
    #[error("恢复密钥无效：{0}")]
    BadRecoveryKey(String),
    #[error("工作空间未解锁")]
    Locked,
    #[error("查看机密需要重新输入访问密码")]
    NeedReauth,
    #[error("已取消生物识别")]
    BiometricCancelled,
    #[error("指纹凭据已失效，请用访问密码解锁后重新开启")]
    BiometricStale,
    #[error("加密/解密失败")]
    Crypto,
    #[error("解锁尝试过于频繁，请稍候再试")]
    RateLimited,
    #[error("IO 错误：{0}")]
    Io(String),
    #[error("序列化错误：{0}")]
    Serde(String),
    #[error("参数错误：{0}")]
    Invalid(String),
    #[error("正在从云端同步，请稍后再修改")]
    Busy,
    /// 当前平台不提供该能力（如移动端没有 ssh-agent / 本地 Git）。
    /// 前端据此隐藏入口，正常路径不应触发，仅作兜底护栏。
    #[error("当前平台不支持该功能：{0}")]
    Unsupported(&'static str),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// 稳定的机器码，供前端做分支与文案本地化。
    pub fn code(&self) -> &'static str {
        match self {
            AppError::AlreadyInitialized(_) => "ALREADY_INITIALIZED",
            AppError::NotInitialized => "NOT_INITIALIZED",
            AppError::BadPassword => "BAD_PASSWORD",
            AppError::BadRecoveryKey(_) => "BAD_RECOVERY_KEY",
            AppError::Locked => "LOCKED",
            AppError::NeedReauth => "NEED_REAUTH",
            AppError::BiometricCancelled => "BIOMETRIC_CANCELLED",
            AppError::BiometricStale => "BIOMETRIC_STALE",
            AppError::Crypto => "CRYPTO",
            AppError::RateLimited => "RATE_LIMITED",
            AppError::Io(_) => "IO",
            AppError::Serde(_) => "SERDE",
            AppError::Invalid(_) => "INVALID",
            AppError::Busy => "BUSY",
            AppError::Unsupported(_) => "UNSUPPORTED_PLATFORM",
            AppError::Other(_) => "OTHER",
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Serde(e.to_string())
    }
}

/// 序列化为 `{ code, message }`，方便前端消费。
impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("AppError", 2)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
