//! 请求追踪模块
//!
//! 定义请求追踪的类型和 trait，用于记录每次 API 请求的详细信息。
//! 数据通过 mpsc channel 异步发送到后台写入任务，避免在请求热路径上同步 IO。

use serde::Serialize;

/// 单次重试尝试记录
#[derive(Debug, Clone, Serialize)]
pub struct TraceAttempt {
    /// 重试序号（从 1 开始）
    pub try_number: i32,
    /// 使用的凭据 ID
    pub credential_id: u64,
    /// HTTP 状态码
    pub status_code: i32,
    /// 结果分类
    pub outcome: AttemptOutcome,
    /// 耗时（毫秒）
    pub duration_ms: i64,
    /// 错误信息（截断到 300 字符）
    pub error: Option<String>,
}

/// 重试尝试结果分类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptOutcome {
    /// 成功
    Success,
    /// 配额耗尽
    QuotaExhausted,
    /// 账户限流
    AccountThrottled,
    /// 认证失败
    AuthFailed,
    /// 瞬态错误（5xx / 网络）
    Transient,
    /// 网络错误
    NetworkError,
    /// 请求格式错误（400）
    BadRequest,
    /// 流被中断
    StreamInterrupted,
    /// 未知
    Unknown,
}

/// 请求追踪记录（完整请求生命周期）
#[derive(Debug, Clone, Serialize)]
pub struct TraceRecord {
    /// 请求路径（如 /v1/messages）
    pub path: String,
    /// 请求的 Anthropic 模型名
    pub model: Option<String>,
    /// 是否流式
    pub is_stream: bool,
    /// 最终 HTTP 状态码
    pub final_status: i32,
    /// 最终使用的凭据 ID
    pub final_credential_id: u64,
    /// 总耗时（毫秒）
    pub duration_ms: i64,
    /// 输入 tokens（估算值）
    pub input_tokens: Option<i32>,
    /// 输出 tokens
    pub output_tokens: Option<i32>,
    /// 总尝试次数
    pub total_attempts: i32,
    /// 错误信息（仅最终失败时有值）
    pub error: Option<String>,
    /// 每次重试的详细记录
    pub attempts: Vec<TraceAttempt>,
}

impl TraceRecord {
    /// 截断错误信息到 300 字符
    pub fn truncate_error(error: &str) -> String {
        if error.len() <= 300 {
            error.to_string()
        } else {
            format!("{}...", &error[..297])
        }
    }
}
