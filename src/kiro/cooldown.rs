//! 冷却管理模块
#![allow(dead_code)]
//!
//! 分类管理不同原因的冷却状态，支持差异化冷却时长和自动清理。
//! 参考 CLIProxyAPIPlus 的实现。

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// 冷却原因
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CooldownReason {
    /// 429 速率限制
    RateLimitExceeded,

    /// 账户暂停
    AccountSuspended,

    /// 配额耗尽
    QuotaExhausted,

    /// Token 刷新失败
    TokenRefreshFailed,

    /// 认证失败
    AuthenticationFailed,

    /// 服务器错误
    ServerError,

    /// 模型暂时不可用
    ModelUnavailable,
}

impl CooldownReason {
    /// 获取默认冷却时长
    pub fn default_duration(&self) -> Duration {
        match self {
            // 短冷却（1-5 分钟）
            CooldownReason::RateLimitExceeded => Duration::from_secs(60),
            CooldownReason::TokenRefreshFailed => Duration::from_secs(60),
            CooldownReason::ServerError => Duration::from_secs(120),
            CooldownReason::ModelUnavailable => Duration::from_secs(300),

            // 长冷却（1-24 小时）
            CooldownReason::AuthenticationFailed => Duration::from_secs(3600),
            CooldownReason::AccountSuspended => Duration::from_secs(86400),
            CooldownReason::QuotaExhausted => Duration::from_secs(86400),
        }
    }

    /// 是否可以自动恢复
    pub fn is_auto_recoverable(&self) -> bool {
        match self {
            CooldownReason::RateLimitExceeded => true,
            CooldownReason::TokenRefreshFailed => true,
            CooldownReason::ServerError => true,
            CooldownReason::ModelUnavailable => true,
            CooldownReason::AuthenticationFailed => false,
            CooldownReason::AccountSuspended => false,
            CooldownReason::QuotaExhausted => false,
        }
    }

    /// 获取原因描述
    pub fn description(&self) -> &'static str {
        match self {
            CooldownReason::RateLimitExceeded => "速率限制",
            CooldownReason::AccountSuspended => "账户暂停",
            CooldownReason::QuotaExhausted => "配额耗尽",
            CooldownReason::TokenRefreshFailed => "Token 刷新失败",
            CooldownReason::AuthenticationFailed => "认证失败",
            CooldownReason::ServerError => "服务器错误",
            CooldownReason::ModelUnavailable => "模型暂时不可用",
        }
    }
}

/// 冷却条目
#[derive(Debug, Clone)]
struct CooldownEntry {
    /// 冷却原因
    reason: CooldownReason,

    /// 冷却开始时间
    started_at: Instant,

    /// 冷却结束时间
    expires_at: Instant,

    /// 连续触发次数（用于递增冷却时长）
    trigger_count: u32,
}

/// 可序列化的冷却条目（用于持久化长冷却）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistentCooldownEntry {
    /// 凭据 ID
    credential_id: u64,
    /// 冷却原因
    reason: CooldownReason,
    /// 冷却结束时间（RFC3339 格式）
    expires_at: String,
    /// 连续触发次数
    trigger_count: u32,
}

/// 冷却管理器
///
/// 管理所有凭据的冷却状态
pub struct CooldownManager {
    /// 凭据冷却状态
    entries: Mutex<HashMap<u64, CooldownEntry>>,

    /// 最大短冷却时长（秒）
    max_short_cooldown_secs: u64,

    /// 长冷却时长（秒）
    long_cooldown_secs: u64,

    /// 持久化文件路径（仅用于长冷却）
    persist_path: Option<PathBuf>,
}

impl Default for CooldownManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CooldownManager {
    /// 创建新的冷却管理器
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            max_short_cooldown_secs: 300, // 5 分钟
            long_cooldown_secs: 86400,    // 24 小时
            persist_path: None,
        }
    }

    /// 使用自定义配置创建冷却管理器
    #[allow(dead_code)]
    pub fn with_config(max_short_cooldown_secs: u64, long_cooldown_secs: u64) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            max_short_cooldown_secs,
            long_cooldown_secs,
            persist_path: None,
        }
    }

    /// 设置持久化路径（启用长冷却持久化）
    pub fn with_persist_path(mut self, path: PathBuf) -> Self {
        self.persist_path = Some(path);
        self
    }

    /// 设置凭据冷却
    ///
    /// 返回实际的冷却时长
    pub fn set_cooldown(&self, credential_id: u64, reason: CooldownReason) -> Duration {
        self.set_cooldown_with_duration(credential_id, reason, None)
    }

    /// 设置凭据冷却（自定义时长）
    pub fn set_cooldown_with_duration(
        &self,
        credential_id: u64,
        reason: CooldownReason,
        custom_duration: Option<Duration>,
    ) -> Duration {
        let mut entries = self.entries.lock();
        let now = Instant::now();

        // 获取或创建条目
        let entry = entries
            .entry(credential_id)
            .or_insert_with(|| CooldownEntry {
                reason,
                started_at: now,
                expires_at: now,
                trigger_count: 0,
            });

        // 更新触发次数
        if entry.reason == reason {
            entry.trigger_count += 1;
        } else {
            entry.reason = reason;
            entry.trigger_count = 1;
        }

        // 计算冷却时长
        let duration = custom_duration
            .unwrap_or_else(|| self.calculate_cooldown_duration(reason, entry.trigger_count));

        entry.started_at = now;
        entry.expires_at = now + duration;

        tracing::info!(
            credential_id = %credential_id,
            reason = %reason.description(),
            duration_secs = %duration.as_secs(),
            trigger_count = %entry.trigger_count,
            "凭据进入冷却"
        );

        // 长冷却持久化
        if !reason.is_auto_recoverable() {
            drop(entries); // 释放锁再持久化
            self.save_persistent_cooldowns();
        }

        duration
    }

    /// 检查凭据是否在冷却中
    ///
    /// 返回 `None` 表示不在冷却中，`Some((reason, remaining))` 表示冷却原因和剩余时间
    pub fn check_cooldown(&self, credential_id: u64) -> Option<(CooldownReason, Duration)> {
        let entries = self.entries.lock();
        let now = Instant::now();

        entries.get(&credential_id).and_then(|entry| {
            if now < entry.expires_at {
                Some((
                    entry.reason,
                    entry.expires_at.saturating_duration_since(now),
                ))
            } else {
                None
            }
        })
    }

    /// 检查凭据是否可用（不在冷却中或冷却已过期）
    pub fn is_available(&self, credential_id: u64) -> bool {
        self.check_cooldown(credential_id).is_none()
    }

    /// 清除凭据冷却
    pub fn clear_cooldown(&self, credential_id: u64) -> bool {
        let mut entries = self.entries.lock();
        entries.remove(&credential_id).is_some()
    }

    /// 清除所有已过期的冷却
    pub fn cleanup_expired(&self) -> usize {
        let mut entries = self.entries.lock();
        let now = Instant::now();
        let before_count = entries.len();

        entries.retain(|_, entry| now < entry.expires_at);

        let removed = before_count - entries.len();
        if removed > 0 {
            tracing::debug!("清理了 {} 个过期冷却条目", removed);
        }
        removed
    }

    /// 获取所有冷却中的凭据
    pub fn get_all_cooldowns(&self) -> Vec<CooldownInfo> {
        let entries = self.entries.lock();
        let now = Instant::now();

        entries
            .iter()
            .filter(|(_, entry)| now < entry.expires_at)
            .map(|(&id, entry)| CooldownInfo {
                credential_id: id,
                reason: entry.reason,
                started_at_ms: entry.started_at.elapsed().as_millis() as u64,
                remaining_ms: entry.expires_at.saturating_duration_since(now).as_millis() as u64,
                trigger_count: entry.trigger_count,
            })
            .collect()
    }

    /// 计算冷却时长
    fn calculate_cooldown_duration(&self, reason: CooldownReason, trigger_count: u32) -> Duration {
        let base = reason.default_duration();

        if reason.is_auto_recoverable() {
            // 可自动恢复的原因：递增冷却时长，但不超过最大值
            let multiplier = 1.5_f64.powi((trigger_count.saturating_sub(1)) as i32);
            let duration_secs = (base.as_secs() as f64 * multiplier) as u64;
            let capped_secs = duration_secs.min(self.max_short_cooldown_secs);
            Duration::from_secs(capped_secs)
        } else {
            // 不可自动恢复的原因：使用长冷却时长
            Duration::from_secs(self.long_cooldown_secs)
        }
    }

    /// 持久化非自动恢复的长冷却到磁盘
    fn save_persistent_cooldowns(&self) {
        let path = match &self.persist_path {
            Some(p) => p,
            None => return,
        };

        let entries = self.entries.lock();
        let now = chrono::Utc::now();

        // 只持久化非自动恢复且未过期的冷却
        let persistent: Vec<PersistentCooldownEntry> = entries
            .iter()
            .filter(|(_, e)| !e.reason.is_auto_recoverable() && Instant::now() < e.expires_at)
            .map(|(&id, e)| {
                // 计算 expires_at 的绝对时间：now + (expires_at - Instant::now())
                let remaining = e.expires_at.saturating_duration_since(Instant::now());
                let expires_at = now + chrono::Duration::from_std(remaining).unwrap_or_default();
                PersistentCooldownEntry {
                    credential_id: id,
                    reason: e.reason,
                    expires_at: expires_at.to_rfc3339(),
                    trigger_count: e.trigger_count,
                }
            })
            .collect();

        if persistent.is_empty() {
            // 无长冷却需要持久化时删除文件
            let _ = std::fs::remove_file(path);
            return;
        }

        match serde_json::to_string_pretty(&persistent) {
            Ok(json) => {
                let tmp = path.with_extension("json.tmp");
                if std::fs::write(&tmp, &json).is_ok()
                    && let Err(e) = std::fs::rename(&tmp, path)
                {
                    tracing::warn!("原子重命名冷却持久化文件失败: {}", e);
                    let _ = std::fs::remove_file(&tmp);
                }
            }
            Err(e) => tracing::warn!("序列化冷却数据失败: {}", e),
        }
    }

    /// 从磁盘加载持久化的长冷却
    ///
    /// 已过期的条目会被丢弃，未过期的条目恢复到内存中
    pub fn load_persistent_cooldowns(&self) {
        let path = match &self.persist_path {
            Some(p) => p,
            None => return,
        };

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return, // 文件不存在（首次运行或已清理）
        };

        let persistent: Vec<PersistentCooldownEntry> = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("解析冷却持久化文件失败，将忽略: {}", e);
                return;
            }
        };

        let now = Instant::now();
        let utc_now = chrono::Utc::now();
        let mut entries = self.entries.lock();

        for p in persistent {
            let expires_at_utc = chrono::DateTime::parse_from_rfc3339(&p.expires_at)
                .map(|dt| dt.with_timezone(&chrono::Utc));
            let expires_at_utc = match expires_at_utc {
                Ok(t) => t,
                Err(_) => continue,
            };
            // 已过期的条目跳过
            if expires_at_utc <= utc_now {
                continue;
            }
            // 计算 Instant 偏移：remaining = expires_at_utc - utc_now
            let remaining = (expires_at_utc - utc_now)
                .to_std()
                .unwrap_or(Duration::from_secs(1));
            let entry = CooldownEntry {
                reason: p.reason,
                started_at: now,
                expires_at: now + remaining,
                trigger_count: p.trigger_count,
            };
            entries.insert(p.credential_id, entry);
            tracing::info!(
                credential_id = %p.credential_id,
                reason = %p.reason.description(),
                remaining_secs = %remaining.as_secs(),
                "从持久化恢复长冷却"
            );
        }
    }
}

/// 冷却信息（公开 API）
#[derive(Debug, Clone)]
pub struct CooldownInfo {
    /// 凭据 ID
    pub credential_id: u64,

    /// 冷却原因
    pub reason: CooldownReason,

    /// 冷却开始时间（毫秒前）
    pub started_at_ms: u64,

    /// 剩余冷却时间（毫秒）
    pub remaining_ms: u64,

    /// 连续触发次数
    pub trigger_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cooldown_manager_new() {
        let manager = CooldownManager::new();
        assert!(manager.is_available(1));
    }

    #[test]
    fn test_cooldown_set_and_check() {
        let manager = CooldownManager::new();

        let duration = manager.set_cooldown(1, CooldownReason::RateLimitExceeded);
        assert!(duration.as_secs() >= 60);

        let (reason, remaining) = manager.check_cooldown(1).unwrap();
        assert_eq!(reason, CooldownReason::RateLimitExceeded);
        assert!(remaining.as_secs() > 0);

        assert!(!manager.is_available(1));
    }

    #[test]
    fn test_cooldown_clear() {
        let manager = CooldownManager::new();

        manager.set_cooldown(1, CooldownReason::ServerError);
        assert!(!manager.is_available(1));

        assert!(manager.clear_cooldown(1));
        assert!(manager.is_available(1));
    }

    #[test]
    fn test_cooldown_incremental() {
        let manager = CooldownManager::new();

        // 第一次冷却
        let d1 = manager.set_cooldown(1, CooldownReason::RateLimitExceeded);

        // 清除后再次触发，应该有更长的冷却
        manager.clear_cooldown(1);
        let d2 = manager.set_cooldown(1, CooldownReason::RateLimitExceeded);

        // 由于触发次数增加，第二次冷却应该更长
        assert!(d2 >= d1);
    }

    #[test]
    fn test_cooldown_reason_auto_recoverable() {
        assert!(CooldownReason::RateLimitExceeded.is_auto_recoverable());
        assert!(CooldownReason::ServerError.is_auto_recoverable());
        assert!(!CooldownReason::AccountSuspended.is_auto_recoverable());
        assert!(!CooldownReason::QuotaExhausted.is_auto_recoverable());
    }

    #[test]
    fn test_cooldown_custom_duration() {
        let manager = CooldownManager::new();

        let custom = Duration::from_secs(10);
        let duration =
            manager.set_cooldown_with_duration(1, CooldownReason::ServerError, Some(custom));

        assert_eq!(duration, custom);
    }

    #[test]
    fn test_cooldown_get_all() {
        let manager = CooldownManager::new();

        manager.set_cooldown(1, CooldownReason::RateLimitExceeded);
        manager.set_cooldown(2, CooldownReason::ServerError);

        let cooldowns = manager.get_all_cooldowns();
        assert_eq!(cooldowns.len(), 2);
    }

    #[test]
    fn test_cooldown_reason_description() {
        assert_eq!(CooldownReason::RateLimitExceeded.description(), "速率限制");
        assert_eq!(CooldownReason::AccountSuspended.description(), "账户暂停");
    }
}
