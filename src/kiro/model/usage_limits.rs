//! 使用额度查询数据模型
//!
//! 包含 getUsageLimits API 的响应类型定义

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// 使用额度查询响应
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimitsResponse {
    /// 下次重置日期 (Unix 时间戳)
    #[serde(default)]
    pub next_date_reset: Option<f64>,

    /// 订阅信息
    #[serde(default)]
    pub subscription_info: Option<SubscriptionInfo>,

    /// 使用量明细列表
    #[serde(default)]
    pub usage_breakdown_list: Vec<UsageBreakdown>,
}

/// 订阅信息
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionInfo {
    /// 订阅标题 (KIRO PRO+ / KIRO FREE 等)
    #[serde(default)]
    pub subscription_title: Option<String>,
}

/// 使用量明细
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBreakdown {
    /// 当前使用量
    #[serde(default)]
    pub current_usage: i64,

    /// 当前使用量（精确值）
    #[serde(default)]
    pub current_usage_with_precision: f64,
    /// 免费试用信息
    #[serde(default)]
    pub free_trial_info: Option<FreeTrialInfo>,

    /// 下次重置日期 (Unix 时间戳)
    #[serde(default)]
    pub next_date_reset: Option<f64>,

    /// 使用限额
    #[serde(default)]
    pub usage_limit: i64,

    /// 使用限额（精确值）
    #[serde(default)]
    pub usage_limit_with_precision: f64,

    /// 额外用量包（如 GIFT 类型）
    #[serde(default)]
    pub bonuses: Option<Vec<Bonus>>,
}

/// 额外用量包信息（如 GIFT 类型）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bonus {
    /// 用量包代码
    #[serde(default)]
    pub bonus_code: Option<String>,

    /// 显示名称
    #[serde(default)]
    pub display_name: Option<String>,

    /// 当前使用量
    #[serde(default)]
    pub current_usage: f64,

    /// 使用限额
    #[serde(default)]
    pub usage_limit: f64,

    /// 状态 (ACTIVE / EXPIRED)
    #[serde(default)]
    pub status: Option<String>,

    /// 过期时间 (Unix 时间戳)
    #[serde(default)]
    pub expires_at: Option<f64>,
}

/// 免费试用信息
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeTrialInfo {
    /// 当前使用量
    #[serde(default)]
    pub current_usage: i64,

    /// 当前使用量（精确值）
    #[serde(default)]
    pub current_usage_with_precision: f64,

    /// 免费试用过期时间 (Unix 时间戳)
    #[serde(default)]
    pub free_trial_expiry: Option<f64>,

    /// 免费试用状态 (ACTIVE / EXPIRED)
    #[serde(default)]
    pub free_trial_status: Option<String>,

    /// 使用限额
    #[serde(default)]
    pub usage_limit: i64,

    /// 使用限额（精确值）
    #[serde(default)]
    pub usage_limit_with_precision: f64,
}

// ============ 便捷方法实现 ============

impl Bonus {
    /// 检查用量包是否处于激活状态
    pub fn is_active(&self) -> bool {
        match self.status.as_deref() {
            Some(s) => s.eq_ignore_ascii_case("ACTIVE"),
            None => {
                // 没有 status 时：优先用 expires_at 判断是否仍有效；再用 limit/current 兜底。
                if let Some(exp) = self.expires_at {
                    let now = Utc::now().timestamp() as f64;
                    return exp > now;
                }
                self.usage_limit > 0.0 || self.current_usage > 0.0
            }
        }
    }
}

impl FreeTrialInfo {
    /// 检查免费试用是否处于激活状态
    pub fn is_active(&self) -> bool {
        self.free_trial_status
            .as_deref()
            .map(|s| s == "ACTIVE")
            .unwrap_or(false)
    }
}

impl UsageLimitsResponse {
    /// 获取订阅标题
    pub fn subscription_title(&self) -> Option<&str> {
        self.subscription_info
            .as_ref()
            .and_then(|info| info.subscription_title.as_deref())
    }

    /// 获取第一个使用量明细
    fn primary_breakdown(&self) -> Option<&UsageBreakdown> {
        self.usage_breakdown_list.first()
    }

    /// 获取总使用限额（精确值）
    ///
    /// 合并基础额度、免费试用额度（如激活）以及所有激活的用量包（如 GIFT）
    pub fn usage_limit(&self) -> f64 {
        let Some(breakdown) = self.primary_breakdown() else {
            return 0.0;
        };

        let base_limit = breakdown.usage_limit_with_precision;

        // 如果 free trial 处于激活状态，合并额度
        let free_trial_limit = breakdown
            .free_trial_info
            .as_ref()
            .filter(|t| t.is_active())
            .map(|t| t.usage_limit_with_precision)
            .unwrap_or(0.0);

        // 合并所有激活的 bonuses 额度
        let bonuses_limit: f64 = breakdown
            .bonuses
            .as_ref()
            .map(|bs| {
                bs.iter()
                    .filter(|b| b.is_active())
                    .map(|b| b.usage_limit)
                    .sum()
            })
            .unwrap_or(0.0);

        base_limit + free_trial_limit + bonuses_limit
    }

    /// 获取总当前使用量（精确值）
    ///
    /// 合并基础使用量、免费试用使用量（如激活）以及所有激活的用量包（如 GIFT）
    pub fn current_usage(&self) -> f64 {
        let Some(breakdown) = self.primary_breakdown() else {
            return 0.0;
        };

        let base_usage = breakdown.current_usage_with_precision;

        // 如果 free trial 处于激活状态，合并使用量
        let free_trial_usage = breakdown
            .free_trial_info
            .as_ref()
            .filter(|t| t.is_active())
            .map(|t| t.current_usage_with_precision)
            .unwrap_or(0.0);

        // 合并所有激活的 bonuses 使用量
        let bonuses_usage: f64 = breakdown
            .bonuses
            .as_ref()
            .map(|bs| {
                bs.iter()
                    .filter(|b| b.is_active())
                    .map(|b| b.current_usage)
                    .sum()
            })
            .unwrap_or(0.0);

        base_usage + free_trial_usage + bonuses_usage
    }
}
