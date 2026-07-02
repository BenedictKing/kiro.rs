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

/// 请求追踪收集器（在 provider retry 循环中填充）
///
/// 由 handler 创建，传递给 provider 的 call_api 方法，
/// provider 在每次重试时调用 `record_attempt`，
/// 最终由 handler 调用 `finish` 生成 TraceRecord。
#[derive(Debug)]
pub struct RequestTrace {
    /// 请求路径
    pub path: String,
    /// 请求的模型名
    pub model: Option<String>,
    /// 是否流式
    pub is_stream: bool,
    /// 请求开始时间
    pub start_time: std::time::Instant,
    /// 收集的 attempt 数据
    pub attempts: Vec<TraceAttempt>,
    /// 最终使用的凭据 ID
    pub final_credential_id: u64,
}

impl RequestTrace {
    /// 创建新的请求追踪器
    pub fn new(path: impl Into<String>, model: Option<String>, is_stream: bool) -> Self {
        Self {
            path: path.into(),
            model,
            is_stream,
            start_time: std::time::Instant::now(),
            attempts: Vec::new(),
            final_credential_id: 0,
        }
    }

    /// 记录一次重试尝试（由 provider 调用）
    pub fn record_attempt(
        &mut self,
        try_number: i32,
        credential_id: u64,
        status_code: i32,
        outcome: AttemptOutcome,
        duration_ms: i64,
        error: Option<String>,
    ) {
        self.attempts.push(TraceAttempt {
            try_number,
            credential_id,
            status_code,
            outcome,
            duration_ms,
            error: error.map(|e| TraceRecord::truncate_error(&e)),
        });
        // 更新最终凭据 ID（最后一次尝试的凭据）
        self.final_credential_id = credential_id;
    }

    /// 生成最终的 TraceRecord
    pub fn finish(
        self,
        final_status: i32,
        input_tokens: Option<i32>,
        output_tokens: Option<i32>,
        error: Option<String>,
    ) -> TraceRecord {
        let total_attempts = self.attempts.len() as i32;
        TraceRecord {
            path: self.path,
            model: self.model,
            is_stream: self.is_stream,
            final_status,
            final_credential_id: self.final_credential_id,
            duration_ms: self.start_time.elapsed().as_millis() as i64,
            input_tokens,
            output_tokens,
            total_attempts,
            error: error.map(|e| TraceRecord::truncate_error(&e)),
            attempts: self.attempts,
        }
    }
}

impl TraceRecord {
    /// 截断错误信息到 300 字节（按字符边界安全截断，避免多字节字符切割 panic）
    pub fn truncate_error(error: &str) -> String {
        if error.len() <= 300 {
            error.to_string()
        } else {
            let end = crate::common::utf8::floor_char_boundary(error, 297);
            format!("{}...", &error[..end])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_error_short() {
        assert_eq!(TraceRecord::truncate_error("短错误"), "短错误");
    }

    #[test]
    fn test_truncate_error_long_ascii() {
        let long = "e".repeat(400);
        let result = TraceRecord::truncate_error(&long);
        assert_eq!(result.len(), 300);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_error_multibyte_no_panic() {
        // 中文每字 3 字节，297 不是 3 的倍数时必然切在字符中间
        let long = "凭证已过期或无效".repeat(30);
        assert!(long.len() > 300);
        let result = TraceRecord::truncate_error(&long);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 300);
    }
}
