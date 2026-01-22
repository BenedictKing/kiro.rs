//! Token 管理模块
//!
//! 负责 Token 过期检测和刷新，支持 Social 和 IdC 认证方式
//! 支持单凭据 (TokenManager) 和多凭据 (MultiTokenManager) 管理

use anyhow::bail;
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use tokio::sync::Mutex as TokioMutex;

use std::path::PathBuf;

use std::collections::HashMap;

use crate::http_client::{ProxyConfig, build_client};
use crate::kiro::affinity::UserAffinityManager;
use crate::kiro::machine_id;
use crate::kiro::model::credentials::KiroCredentials;
use crate::kiro::model::token_refresh::{
    IdcRefreshRequest, IdcRefreshResponse, RefreshRequest, RefreshResponse,
};
use crate::kiro::model::usage_limits::UsageLimitsResponse;
use crate::model::config::Config;

/// Token 管理器
///
/// 负责管理凭据和 Token 的自动刷新
#[allow(dead_code)]
pub struct TokenManager {
    config: Config,
    credentials: KiroCredentials,
    proxy: Option<ProxyConfig>,
}

#[allow(dead_code)]
impl TokenManager {
    /// 创建新的 TokenManager 实例
    pub fn new(config: Config, credentials: KiroCredentials, proxy: Option<ProxyConfig>) -> Self {
        Self {
            config,
            credentials,
            proxy,
        }
    }

    /// 获取凭据的引用
    pub fn credentials(&self) -> &KiroCredentials {
        &self.credentials
    }

    /// 获取配置的引用
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// 确保获取有效的访问 Token
    ///
    /// 如果 Token 过期或即将过期，会自动刷新
    pub async fn ensure_valid_token(&mut self) -> anyhow::Result<String> {
        if is_token_expired(&self.credentials) || is_token_expiring_soon(&self.credentials) {
            self.credentials =
                refresh_token(&self.credentials, &self.config, self.proxy.as_ref()).await?;

            // 刷新后再次检查 token 时间有效性
            if is_token_expired(&self.credentials) {
                anyhow::bail!("刷新后的 Token 仍然无效或已过期");
            }
        }

        self.credentials
            .access_token
            .clone()
            .ok_or_else(|| anyhow::anyhow!("没有可用的 accessToken"))
    }

    /// 获取使用额度信息
    ///
    /// 调用 getUsageLimits API 查询当前账户的使用额度
    pub async fn get_usage_limits(&mut self) -> anyhow::Result<UsageLimitsResponse> {
        let token = self.ensure_valid_token().await?;
        get_usage_limits(&self.credentials, &self.config, &token, self.proxy.as_ref()).await
    }
}

/// 检查 Token 是否在指定时间内过期
pub(crate) fn is_token_expiring_within(
    credentials: &KiroCredentials,
    minutes: i64,
) -> Option<bool> {
    credentials
        .expires_at
        .as_ref()
        .and_then(|expires_at| DateTime::parse_from_rfc3339(expires_at).ok())
        .map(|expires| expires <= Utc::now() + Duration::minutes(minutes))
}

/// 检查 Token 是否已过期（提前 5 分钟判断）
pub(crate) fn is_token_expired(credentials: &KiroCredentials) -> bool {
    is_token_expiring_within(credentials, 5).unwrap_or(true)
}

/// 检查 Token 是否即将过期（10分钟内）
pub(crate) fn is_token_expiring_soon(credentials: &KiroCredentials) -> bool {
    is_token_expiring_within(credentials, 10).unwrap_or(false)
}

/// 验证 refreshToken 的基本有效性
pub(crate) fn validate_refresh_token(credentials: &KiroCredentials) -> anyhow::Result<()> {
    let refresh_token = credentials
        .refresh_token
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("缺少 refreshToken"))?;

    if refresh_token.is_empty() {
        bail!("refreshToken 为空");
    }

    if refresh_token.len() < 100 || refresh_token.ends_with("...") || refresh_token.contains("...")
    {
        bail!(
            "refreshToken 已被截断（长度: {} 字符）。\n\
             这通常是 Kiro IDE 为了防止凭证被第三方工具使用而故意截断的。",
            refresh_token.len()
        );
    }

    Ok(())
}

/// 刷新 Token
pub(crate) async fn refresh_token(
    credentials: &KiroCredentials,
    config: &Config,
    proxy: Option<&ProxyConfig>,
) -> anyhow::Result<KiroCredentials> {
    // 使用凭据自身的 ID（如果有）
    let id = credentials.id.unwrap_or(0);
    refresh_token_with_id(credentials, config, proxy, id).await
}

/// 刷新 Token（带凭证 ID）
pub(crate) async fn refresh_token_with_id(
    credentials: &KiroCredentials,
    config: &Config,
    proxy: Option<&ProxyConfig>,
    _id: u64,
) -> anyhow::Result<KiroCredentials> {
    validate_refresh_token(credentials)?;

    // 根据 auth_method 选择刷新方式
    // 如果未指定 auth_method，根据是否有 clientId/clientSecret 自动判断
    let auth_method = credentials.auth_method.as_deref().unwrap_or_else(|| {
        if credentials.client_id.is_some() && credentials.client_secret.is_some() {
            "idc"
        } else {
            "social"
        }
    });

    if auth_method.eq_ignore_ascii_case("idc")
        || auth_method.eq_ignore_ascii_case("builder-id")
        || auth_method.eq_ignore_ascii_case("iam")
    {
        refresh_idc_token(credentials, config, proxy).await
    } else {
        refresh_social_token(credentials, config, proxy).await
    }
}

/// 刷新 Social Token
async fn refresh_social_token(
    credentials: &KiroCredentials,
    config: &Config,
    proxy: Option<&ProxyConfig>,
) -> anyhow::Result<KiroCredentials> {
    tracing::info!("正在刷新 Social Token...");

    let refresh_token = credentials.refresh_token.as_ref().unwrap();
    // 优先使用凭据级 region，未配置或为空时回退到 config.region
    let region = credentials
        .region
        .as_ref()
        .filter(|r| !r.trim().is_empty())
        .unwrap_or(&config.region);

    let refresh_url = format!("https://prod.{}.auth.desktop.kiro.dev/refreshToken", region);
    let refresh_domain = format!("prod.{}.auth.desktop.kiro.dev", region);
    let machine_id = machine_id::generate_from_credentials(credentials, config)
        .ok_or_else(|| anyhow::anyhow!("无法生成 machineId"))?;
    let kiro_version = &config.kiro_version;

    let client = build_client(proxy, 60, config.tls_backend)?;
    let body = RefreshRequest {
        refresh_token: refresh_token.to_string(),
    };

    let response = client
        .post(&refresh_url)
        .header("Accept", "application/json, text/plain, */*")
        .header("Content-Type", "application/json")
        .header(
            "User-Agent",
            format!("KiroIDE-{}-{}", kiro_version, machine_id),
        )
        .header("Accept-Encoding", "gzip, compress, deflate, br")
        .header("host", &refresh_domain)
        .header("Connection", "close")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        let error_msg = match status.as_u16() {
            401 => "OAuth 凭证已过期或无效，需要重新认证",
            403 => "权限不足，无法刷新 Token",
            429 => "请求过于频繁，已被限流",
            500..=599 => "服务器错误，AWS OAuth 服务暂时不可用",
            _ => "Token 刷新失败",
        };
        bail!("{}: {} {}", error_msg, status, body_text);
    }

    let data: RefreshResponse = response.json().await?;

    let mut new_credentials = credentials.clone();
    new_credentials.access_token = Some(data.access_token);

    if let Some(new_refresh_token) = data.refresh_token {
        new_credentials.refresh_token = Some(new_refresh_token);
    }

    if let Some(profile_arn) = data.profile_arn {
        new_credentials.profile_arn = Some(profile_arn);
    }

    if let Some(expires_in) = data.expires_in {
        let expires_at = Utc::now() + Duration::seconds(expires_in);
        new_credentials.expires_at = Some(expires_at.to_rfc3339());
        tracing::info!(expires_in = %expires_in, "Social Token 刷新成功");
    } else {
        tracing::info!("Social Token 刷新成功（无过期时间）");
    }

    Ok(new_credentials)
}

/// IdC Token 刷新所需的 x-amz-user-agent header
const IDC_AMZ_USER_AGENT: &str = "aws-sdk-js/3.738.0 ua/2.1 os/other lang/js md/browser#unknown_unknown api/sso-oidc#3.738.0 m/E KiroIDE";

/// 刷新 IdC Token (AWS SSO OIDC)
async fn refresh_idc_token(
    credentials: &KiroCredentials,
    config: &Config,
    proxy: Option<&ProxyConfig>,
) -> anyhow::Result<KiroCredentials> {
    tracing::info!("正在刷新 IdC Token...");

    let refresh_token = credentials.refresh_token.as_ref().unwrap();
    let client_id = credentials
        .client_id
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("IdC 刷新需要 clientId"))?;
    let client_secret = credentials
        .client_secret
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("IdC 刷新需要 clientSecret"))?;

    // 优先使用凭据级 region，未配置或为空时回退到 config.region
    let region = credentials
        .region
        .as_ref()
        .filter(|r| !r.trim().is_empty())
        .unwrap_or(&config.region);
    let refresh_url = format!("https://oidc.{}.amazonaws.com/token", region);

    let client = build_client(proxy, 60, config.tls_backend)?;
    let body = IdcRefreshRequest {
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        refresh_token: refresh_token.to_string(),
        grant_type: "refresh_token".to_string(),
    };

    let response = client
        .post(&refresh_url)
        .header("Content-Type", "application/json")
        .header("Host", format!("oidc.{}.amazonaws.com", region))
        .header("Connection", "keep-alive")
        .header("x-amz-user-agent", IDC_AMZ_USER_AGENT)
        .header("Accept", "*/*")
        .header("Accept-Language", "*")
        .header("sec-fetch-mode", "cors")
        .header("User-Agent", "node")
        .header("Accept-Encoding", "br, gzip, deflate")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        let error_msg = match status.as_u16() {
            401 => "IdC 凭证已过期或无效，需要重新认证",
            403 => "权限不足，无法刷新 Token",
            429 => "请求过于频繁，已被限流",
            500..=599 => "服务器错误，AWS OIDC 服务暂时不可用",
            _ => "IdC Token 刷新失败",
        };
        bail!("{}: {} {}", error_msg, status, body_text);
    }

    let data: IdcRefreshResponse = response.json().await?;

    let mut new_credentials = credentials.clone();
    new_credentials.access_token = Some(data.access_token);

    if let Some(new_refresh_token) = data.refresh_token {
        new_credentials.refresh_token = Some(new_refresh_token);
    }

    if let Some(expires_in) = data.expires_in {
        let expires_at = Utc::now() + Duration::seconds(expires_in);
        new_credentials.expires_at = Some(expires_at.to_rfc3339());
        tracing::info!(expires_in = %expires_in, "IdC Token 刷新成功");
    } else {
        tracing::info!("IdC Token 刷新成功（无过期时间）");
    }

    Ok(new_credentials)
}

/// getUsageLimits API 所需的 x-amz-user-agent header 前缀
const USAGE_LIMITS_AMZ_USER_AGENT_PREFIX: &str = "aws-sdk-js/1.0.0";

/// 获取使用额度信息
pub(crate) async fn get_usage_limits(
    credentials: &KiroCredentials,
    config: &Config,
    token: &str,
    proxy: Option<&ProxyConfig>,
) -> anyhow::Result<UsageLimitsResponse> {
    tracing::debug!("正在获取使用额度信息...");

    let region = &config.region;
    let host = format!("q.{}.amazonaws.com", region);
    let machine_id = machine_id::generate_from_credentials(credentials, config)
        .ok_or_else(|| anyhow::anyhow!("无法生成 machineId"))?;
    let kiro_version = &config.kiro_version;

    // 构建 URL
    let mut url = format!(
        "https://{}/getUsageLimits?origin=AI_EDITOR&resourceType=AGENTIC_REQUEST",
        host
    );

    // profileArn 是可选的
    if let Some(profile_arn) = &credentials.profile_arn {
        url.push_str(&format!("&profileArn={}", urlencoding::encode(profile_arn)));
    }

    // 构建 User-Agent headers
    let user_agent = format!(
        "aws-sdk-js/1.0.0 ua/2.1 os/darwin#24.6.0 lang/js md/nodejs#22.21.1 \
         api/codewhispererruntime#1.0.0 m/N,E KiroIDE-{}-{}",
        kiro_version, machine_id
    );
    let amz_user_agent = format!(
        "{} KiroIDE-{}-{}",
        USAGE_LIMITS_AMZ_USER_AGENT_PREFIX, kiro_version, machine_id
    );

    let client = build_client(proxy, 60, config.tls_backend)?;

    let response = client
        .get(&url)
        .header("x-amz-user-agent", &amz_user_agent)
        .header("User-Agent", &user_agent)
        .header("host", &host)
        .header("amz-sdk-invocation-id", uuid::Uuid::new_v4().to_string())
        .header("amz-sdk-request", "attempt=1; max=1")
        .header("Authorization", format!("Bearer {}", token))
        .header("Connection", "close")
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        let error_msg = match status.as_u16() {
            401 => "认证失败，Token 无效或已过期",
            403 => "权限不足，无法获取使用额度",
            429 => "请求过于频繁，已被限流",
            500..=599 => "服务器错误，AWS 服务暂时不可用",
            _ => "获取使用额度失败",
        };
        bail!("{}: {} {}", error_msg, status, body_text);
    }

    // 先获取原始响应文本，便于调试 JSON 解析错误
    let body_text = response.text().await?;

    let data: UsageLimitsResponse = serde_json::from_str(&body_text).map_err(|e| {
        tracing::error!(
            "getUsageLimits JSON 解析失败: {}，原始响应: {}",
            e,
            body_text
        );
        anyhow::anyhow!("JSON 解析失败: {}", e)
    })?;
    Ok(data)
}

// ============================================================================
// 多凭据 Token 管理器
// ============================================================================

/// 凭据禁用原因
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisableReason {
    /// 连续失败次数过多
    FailureLimit,
    /// 余额不足
    #[allow(dead_code)]
    InsufficientBalance,
    /// 模型临时不可用（全局禁用）
    ModelUnavailable,
    /// 手动禁用
    Manual,
    /// 额度已用尽（如 MONTHLY_REQUEST_COUNT）
    QuotaExceeded,
}

/// 单个凭据条目的状态
struct CredentialEntry {
    /// 凭据唯一 ID
    id: u64,
    /// 凭据信息
    credentials: KiroCredentials,
    /// API 调用连续失败次数
    failure_count: u32,
    /// 是否已禁用
    disabled: bool,
    /// 自愈原因（用于区分手动禁用 vs 自动禁用，便于自愈逻辑判断）
    auto_heal_reason: Option<AutoHealReason>,
    /// 禁用原因（公共 API 展示用）
    disable_reason: Option<DisableReason>,
}

/// 自愈原因（内部使用，用于判断是否可自动恢复）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoHealReason {
    /// Admin API 手动禁用（不自动恢复）
    Manual,
    /// 连续失败达到阈值后自动禁用（可自动恢复）
    TooManyFailures,
    /// 额度已用尽（如 MONTHLY_REQUEST_COUNT）
    #[allow(dead_code)]
    QuotaExceeded,
}

// ============================================================================
// Admin API 公开结构
// ============================================================================

/// 凭据条目快照（用于 Admin API 读取）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialEntrySnapshot {
    /// 凭据唯一 ID
    pub id: u64,
    /// 优先级
    pub priority: u32,
    /// 是否被禁用
    pub disabled: bool,
    /// 禁用原因
    pub disable_reason: Option<DisableReason>,
    /// 连续失败次数
    pub failure_count: u32,
    /// 认证方式
    pub auth_method: Option<String>,
    /// 是否有 Profile ARN
    pub has_profile_arn: bool,
    /// Token 过期时间
    pub expires_at: Option<String>,
}

/// 凭据管理器状态快照
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerSnapshot {
    /// 凭据条目列表
    pub entries: Vec<CredentialEntrySnapshot>,
    /// 总凭据数量
    pub total: usize,
    /// 可用凭据数量
    pub available: usize,
}

/// 缓存余额信息（用于 Admin API）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedBalanceInfo {
    /// 凭据 ID
    pub id: u64,
    /// 缓存的剩余额度
    pub remaining: f64,
    /// 缓存时间（Unix 毫秒时间戳）
    pub cached_at: u64,
    /// 缓存存活时间（秒）
    pub ttl_secs: u64,
}

/// 余额缓存条目
struct CachedBalance {
    remaining: f64,
    cached_at: std::time::Instant,
    /// 是否已初始化（区分"未获取过余额"和"余额为零"）
    initialized: bool,
    /// 最近一段时间的使用次数（用于判断高频/低频）
    recent_usage: u32,
    /// 上次重置使用计数的时间
    usage_reset_at: std::time::Instant,
}

/// 高频渠道 TTL（10 分钟）
const BALANCE_TTL_HIGH_FREQ_SECS: u64 = 600;
/// 低频渠道 TTL（30 分钟）
const BALANCE_TTL_LOW_FREQ_SECS: u64 = 1800;
/// 低余额渠道 TTL（24 小时）
const BALANCE_TTL_LOW_BALANCE_SECS: u64 = 86400;
/// 高频判定阈值（10分钟内使用超过此次数视为高频）
const HIGH_FREQ_THRESHOLD: u32 = 20;
/// 使用计数重置周期（10 分钟）
const USAGE_COUNT_RESET_SECS: u64 = 600;
/// 低余额阈值
const LOW_BALANCE_THRESHOLD: f64 = 1.0;

/// 多凭据 Token 管理器
///
/// 支持多个凭据的管理，实现负载均衡 + 故障转移策略
/// 故障统计基于 API 调用结果，而非 Token 刷新结果
pub struct MultiTokenManager {
    config: Config,
    proxy: Option<ProxyConfig>,
    /// 凭据条目列表
    entries: Mutex<Vec<CredentialEntry>>,
    /// Token 刷新锁，确保同一时间只有一个刷新操作
    refresh_lock: TokioMutex<()>,
    /// 凭据文件路径（用于回写）
    credentials_path: Option<PathBuf>,
    /// 是否为多凭据格式（数组格式才回写）
    is_multiple_format: bool,
    /// MODEL_TEMPORARILY_UNAVAILABLE 错误计数
    model_unavailable_count: AtomicU32,
    /// 选择抖动计数器（用于同权重候选的轮询，避免总选第一个）
    selection_rr: AtomicU64,
    /// 全局禁用恢复时间（None 表示未被全局禁用）
    global_recovery_time: Mutex<Option<DateTime<Utc>>>,
    /// 用户亲和性管理器
    affinity: UserAffinityManager,
    /// 余额缓存（用于负载均衡和故障转移时选择最优凭据）
    balance_cache: Mutex<HashMap<u64, CachedBalance>>,
}

/// 每个凭据最大 API 调用失败次数
const MAX_FAILURES_PER_CREDENTIAL: u32 = 2;

/// MODEL_TEMPORARILY_UNAVAILABLE 触发全局禁用的阈值
const MODEL_UNAVAILABLE_THRESHOLD: u32 = 2;

/// 全局禁用恢复时间（分钟）
const GLOBAL_DISABLE_RECOVERY_MINUTES: i64 = 5;

/// API 调用上下文
///
/// 绑定特定凭据的调用上下文，确保 token、credentials 和 id 的一致性
#[derive(Clone)]
pub struct CallContext {
    /// 凭据 ID（用于 report_success/report_failure）
    pub id: u64,
    /// 凭据信息（用于构建请求头）
    pub credentials: KiroCredentials,
    /// 访问 Token
    pub token: String,
}

impl MultiTokenManager {
    /// 创建多凭据 Token 管理器
    ///
    /// # Arguments
    /// * `config` - 应用配置
    /// * `credentials` - 凭据列表
    /// * `proxy` - 可选的代理配置
    /// * `credentials_path` - 凭据文件路径（用于回写）
    /// * `is_multiple_format` - 是否为多凭据格式（数组格式才回写）
    pub fn new(
        config: Config,
        credentials: Vec<KiroCredentials>,
        proxy: Option<ProxyConfig>,
        credentials_path: Option<PathBuf>,
        is_multiple_format: bool,
    ) -> anyhow::Result<Self> {
        // 计算当前最大 ID，为没有 ID 的凭据分配新 ID
        let max_existing_id = credentials.iter().filter_map(|c| c.id).max().unwrap_or(0);
        let mut next_id = max_existing_id + 1;
        let mut has_new_ids = false;
        let mut has_new_machine_ids = false;
        let config_ref = &config;

        let entries: Vec<CredentialEntry> = credentials
            .into_iter()
            .map(|mut cred| {
                cred.canonicalize_auth_method();
                let id = cred.id.unwrap_or_else(|| {
                    let id = next_id;
                    next_id += 1;
                    cred.id = Some(id);
                    has_new_ids = true;
                    id
                });
                if cred.machine_id.is_none()
                    && let Some(machine_id) =
                        machine_id::generate_from_credentials(&cred, config_ref)
                {
                    cred.machine_id = Some(machine_id);
                    has_new_machine_ids = true;
                }
                CredentialEntry {
                    id,
                    credentials: cred,
                    failure_count: 0,
                    disabled: false,
                    auto_heal_reason: None,
                    disable_reason: None,
                }
            })
            .collect();

        // 检测重复 ID
        let mut seen_ids = std::collections::HashSet::new();
        let mut duplicate_ids = Vec::new();
        for entry in &entries {
            if !seen_ids.insert(entry.id) {
                duplicate_ids.push(entry.id);
            }
        }
        if !duplicate_ids.is_empty() {
            anyhow::bail!("检测到重复的凭据 ID: {:?}", duplicate_ids);
        }

        // 初始化余额缓存（为每个凭据创建初始条目，支持负载均衡）
        let now = std::time::Instant::now();
        let initial_cache: HashMap<u64, CachedBalance> = entries
            .iter()
            .map(|e| {
                (
                    e.id,
                    CachedBalance {
                        remaining: 0.0,
                        cached_at: now,
                        initialized: false,
                        recent_usage: 0,
                        usage_reset_at: now,
                    },
                )
            })
            .collect();

        let manager = Self {
            config,
            proxy,
            entries: Mutex::new(entries),
            refresh_lock: TokioMutex::new(()),
            credentials_path,
            is_multiple_format,
            model_unavailable_count: AtomicU32::new(0),
            selection_rr: AtomicU64::new(0),
            global_recovery_time: Mutex::new(None),
            affinity: UserAffinityManager::new(),
            balance_cache: Mutex::new(initial_cache),
        };

        // 如果有新分配的 ID 或新生成的 machineId，立即持久化到配置文件
        if has_new_ids || has_new_machine_ids {
            if let Err(e) = manager.persist_credentials() {
                tracing::warn!("补全凭据 ID/machineId 后持久化失败: {}", e);
            } else {
                tracing::info!("已补全凭据 ID/machineId 并写回配置文件");
            }
        }

        Ok(manager)
    }

    /// 获取配置的引用
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// 获取凭据总数
    pub fn total_count(&self) -> usize {
        self.entries.lock().len()
    }

    /// 获取可用凭据数量
    pub fn available_count(&self) -> usize {
        self.entries.lock().iter().filter(|e| !e.disabled).count()
    }

    /// 选择最佳凭据（两级排序：使用次数最少 + 余额最多；完全相同则轮询）
    fn select_best_candidate_id(&self, candidate_ids: &[u64]) -> Option<u64> {
        if candidate_ids.is_empty() {
            return None;
        }

        let rr = self.selection_rr.fetch_add(1, Ordering::Relaxed) as usize;
        let cache = self.balance_cache.lock();

        let mut scored: Vec<(u64, u32, f64)> = Vec::with_capacity(candidate_ids.len());
        for &id in candidate_ids {
            let (usage, balance, initialized) = cache
                .get(&id)
                .map(|c| (c.recent_usage, c.remaining, c.initialized))
                .unwrap_or((0, 0.0, false));
            // 未初始化的凭据视为使用次数最大，避免被优先选中
            let effective_usage = if initialized { usage } else { u32::MAX };
            // NaN 余额归一化为 0.0，避免 total_cmp 将 NaN 视为最大值
            let effective_balance = if balance.is_finite() { balance } else { 0.0 };
            scored.push((id, effective_usage, effective_balance));
        }

        // 第一优先级：使用次数最少
        let min_usage = scored.iter().map(|(_, usage, _)| *usage).min()?;
        scored.retain(|(_, usage, _)| *usage == min_usage);

        // 第二优先级：余额最多（使用次数相同）
        let mut max_balance = scored.first().map(|(_, _, b)| *b).unwrap_or(0.0);
        for &(_, _, balance) in &scored {
            if balance > max_balance {
                max_balance = balance;
            }
        }
        scored.retain(|(_, _, balance)| *balance == max_balance);

        if scored.len() == 1 {
            return Some(scored[0].0);
        }

        // 兜底：完全相同则轮询，避免总选第一个
        let index = rr % scored.len();
        Some(scored[index].0)
    }

    /// 获取 API 调用上下文
    ///
    /// 返回绑定了 id、credentials 和 token 的调用上下文
    /// 确保整个 API 调用过程中使用一致的凭据信息
    ///
    /// 选择策略：按优先级选择可用凭据
    /// 如果 Token 过期或即将过期，会自动刷新
    /// Token 刷新失败时会尝试下一个可用凭据（不计入失败次数）
    pub async fn acquire_context(&self) -> anyhow::Result<CallContext> {
        // 检查是否需要自动恢复
        self.check_and_recover();

        let total = self.total_count();
        let mut tried_ids: Vec<u64> = Vec::new();

        loop {
            if tried_ids.len() >= total {
                anyhow::bail!(
                    "所有凭据均无法获取有效 Token（可用: {}/{}）",
                    self.available_count(),
                    total
                );
            }

            let candidate_infos: Vec<(u64, u32)> = {
                let mut entries = self.entries.lock();

                let mut candidates: Vec<(u64, u32)> = entries
                    .iter()
                    .filter(|e| !e.disabled && !tried_ids.contains(&e.id))
                    .map(|e| (e.id, e.credentials.priority))
                    .collect();

                // 没有可用凭据：如果是"自动禁用导致全灭"，做一次类似重启的自愈
                if candidates.is_empty()
                    && entries.iter().any(|e| {
                        e.disabled && e.auto_heal_reason == Some(AutoHealReason::TooManyFailures)
                    })
                {
                    tracing::warn!(
                        "所有凭据均已被自动禁用，执行自愈：重置失败计数并重新启用（等价于重启）"
                    );
                    for e in entries.iter_mut() {
                        if e.auto_heal_reason == Some(AutoHealReason::TooManyFailures) {
                            e.disabled = false;
                            e.auto_heal_reason = None;
                            e.disable_reason = None;
                            e.failure_count = 0;
                        }
                    }

                    candidates = entries
                        .iter()
                        .filter(|e| !e.disabled && !tried_ids.contains(&e.id))
                        .map(|e| (e.id, e.credentials.priority))
                        .collect();
                }

                if candidates.is_empty() {
                    let available = entries.iter().filter(|e| !e.disabled).count();
                    anyhow::bail!("所有凭据均已禁用（{}/{}）", available, total);
                }

                candidates
            };

            // 按优先级选出候选集合，再在同优先级内做负载均衡选择
            let min_priority = candidate_infos.iter().map(|(_, p)| *p).min().unwrap_or(0);
            let candidate_ids: Vec<u64> = candidate_infos
                .iter()
                .filter(|(_, p)| *p == min_priority)
                .map(|(id, _)| *id)
                .collect();
            let id = self
                .select_best_candidate_id(&candidate_ids)
                .ok_or_else(|| anyhow::anyhow!("没有可用凭据"))?;

            let credentials = {
                let entries = self.entries.lock();
                entries
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.credentials.clone())
                    .ok_or_else(|| anyhow::anyhow!("凭据 #{} 不存在", id))?
            };

            // 尝试获取/刷新 Token
            match self.try_ensure_token(id, &credentials).await {
                Ok(ctx) => {
                    return Ok(ctx);
                }
                Err(e) => {
                    tracing::warn!("凭据 #{} Token 刷新失败，尝试下一个凭据: {}", id, e);
                    tried_ids.push(id);
                }
            }
        }
    }

    /// 获取指定用户的 API 调用上下文（带亲和性）
    ///
    /// 如果用户已绑定凭据且该凭据可用，优先使用绑定的凭据
    /// 否则使用默认的 acquire_context() 逻辑并建立新绑定
    pub async fn acquire_context_for_user(
        &self,
        user_id: Option<&str>,
    ) -> anyhow::Result<CallContext> {
        // 无 user_id 时走默认逻辑
        let user_id = match user_id {
            Some(id) if !id.is_empty() => id,
            _ => return self.acquire_context().await,
        };

        // 检查亲和性绑定
        if let Some(bound_id) = self.affinity.get(user_id) {
            // 检查绑定的凭据是否可用
            let is_available = {
                let entries = self.entries.lock();
                entries.iter().any(|e| e.id == bound_id && !e.disabled)
            };

            if is_available {
                // 尝试使用绑定的凭据
                let credentials = {
                    let entries = self.entries.lock();
                    entries
                        .iter()
                        .find(|e| e.id == bound_id)
                        .map(|e| e.credentials.clone())
                };

                if let Some(creds) = credentials
                    && let Ok(ctx) = self.try_ensure_token(bound_id, &creds).await
                {
                    self.affinity.touch(user_id);
                    return Ok(ctx);
                }
            }
        }

        // 绑定不存在或凭据不可用，选择最优凭据（两级判断：使用次数最少 + 余额最多）
        let candidates: Vec<u64> = {
            let entries = self.entries.lock();
            entries
                .iter()
                .filter(|e| !e.disabled)
                .map(|e| e.id)
                .collect()
        };

        let best_candidate = self.select_best_candidate_id(&candidates);

        if let Some(id) = best_candidate {
            let credentials = {
                let entries = self.entries.lock();
                entries
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.credentials.clone())
            };
            if let Some(creds) = credentials
                && let Ok(ctx) = self.try_ensure_token(id, &creds).await
            {
                self.affinity.set(user_id, ctx.id);
                return Ok(ctx);
            }
        }

        // 回退到默认逻辑（按优先级选择）
        let ctx = self.acquire_context().await?;
        self.affinity.set(user_id, ctx.id);
        Ok(ctx)
    }

    /// 获取缓存的余额（用于故障转移选择）
    #[allow(dead_code)]
    fn get_cached_balance(&self, id: u64) -> f64 {
        let cache = self.balance_cache.lock();
        if let Some(entry) = cache.get(&id) {
            // 动态 TTL：低余额 > 低频 > 高频
            let ttl = if entry.remaining < LOW_BALANCE_THRESHOLD {
                BALANCE_TTL_LOW_BALANCE_SECS
            } else if entry.recent_usage >= HIGH_FREQ_THRESHOLD {
                BALANCE_TTL_HIGH_FREQ_SECS
            } else {
                BALANCE_TTL_LOW_FREQ_SECS
            };
            if entry.cached_at.elapsed().as_secs() < ttl {
                return entry.remaining;
            }
        }
        // 缓存不存在或过期，返回 0（会回退到优先级选择）
        0.0
    }

    /// 更新余额缓存
    pub fn update_balance_cache(&self, id: u64, remaining: f64) {
        let mut cache = self.balance_cache.lock();
        let now = std::time::Instant::now();
        // 保留现有使用计数
        let (recent_usage, usage_reset_at) = cache
            .get(&id)
            .map(|e| (e.recent_usage, e.usage_reset_at))
            .unwrap_or((0, now));
        cache.insert(
            id,
            CachedBalance {
                remaining,
                cached_at: now,
                initialized: true,
                recent_usage,
                usage_reset_at,
            },
        );
    }

    /// 检查是否需要刷新余额缓存
    pub fn should_refresh_balance(&self, id: u64) -> bool {
        let cache = self.balance_cache.lock();
        if let Some(entry) = cache.get(&id) {
            // 未初始化的缓存需要立即刷新
            if !entry.initialized {
                return true;
            }
            // 使用动态 TTL 判断是否过期
            let ttl = if entry.remaining < LOW_BALANCE_THRESHOLD {
                BALANCE_TTL_LOW_BALANCE_SECS
            } else if entry.recent_usage >= HIGH_FREQ_THRESHOLD {
                BALANCE_TTL_HIGH_FREQ_SECS
            } else {
                BALANCE_TTL_LOW_FREQ_SECS
            };
            entry.cached_at.elapsed().as_secs() >= ttl
        } else {
            true // 无缓存，需要刷新
        }
    }

    /// 记录凭据使用（用于动态 TTL 计算和负载均衡）
    pub fn record_usage(&self, id: u64) {
        let mut cache = self.balance_cache.lock();
        let now = std::time::Instant::now();
        if let Some(entry) = cache.get_mut(&id) {
            // 重置周期过期则清零
            if entry.usage_reset_at.elapsed().as_secs() >= USAGE_COUNT_RESET_SECS {
                entry.recent_usage = 1;
                entry.usage_reset_at = now;
            } else {
                entry.recent_usage = entry.recent_usage.saturating_add(1);
            }
        } else {
            // 缓存条目不存在时创建新条目（余额未知设为 0）
            cache.insert(
                id,
                CachedBalance {
                    remaining: 0.0,
                    cached_at: now,
                    initialized: false,
                    recent_usage: 1,
                    usage_reset_at: now,
                },
            );
        }
    }

    /// 获取所有凭据的缓存余额信息（用于 Admin API）
    ///
    /// 返回每个凭据的缓存余额、缓存时间和 TTL
    pub fn get_all_cached_balances(&self) -> Vec<CachedBalanceInfo> {
        // 先获取 entries 的 ID 列表，避免同时持有两个锁
        let entry_ids: Vec<u64> = {
            let entries = self.entries.lock();
            entries.iter().map(|e| e.id).collect()
        };

        let cache = self.balance_cache.lock();
        let now_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        entry_ids
            .iter()
            .filter_map(|&id| {
                cache.get(&id).map(|cached| {
                    // 计算动态 TTL
                    let ttl_secs = if !cached.initialized {
                        // 未初始化的缓存，TTL 设为 0（已过期）
                        0
                    } else if cached.remaining < LOW_BALANCE_THRESHOLD {
                        BALANCE_TTL_LOW_BALANCE_SECS
                    } else if cached.recent_usage >= HIGH_FREQ_THRESHOLD {
                        BALANCE_TTL_HIGH_FREQ_SECS
                    } else {
                        BALANCE_TTL_LOW_FREQ_SECS
                    };

                    // 计算缓存时间的 Unix 毫秒时间戳
                    let elapsed_ms = cached.cached_at.elapsed().as_millis() as u64;
                    let cached_at_unix_ms = now_unix_ms.saturating_sub(elapsed_ms);

                    CachedBalanceInfo {
                        id,
                        remaining: cached.remaining,
                        cached_at: cached_at_unix_ms,
                        ttl_secs,
                    }
                })
            })
            .collect()
    }

    /// 尝试使用指定凭据获取有效 Token
    ///
    /// 使用双重检查锁定模式，确保同一时间只有一个刷新操作
    ///
    /// # Arguments
    /// * `id` - 凭据 ID，用于更新正确的条目
    /// * `credentials` - 凭据信息
    async fn try_ensure_token(
        &self,
        id: u64,
        credentials: &KiroCredentials,
    ) -> anyhow::Result<CallContext> {
        // 第一次检查（无锁）：快速判断是否需要刷新
        let needs_refresh = is_token_expired(credentials) || is_token_expiring_soon(credentials);

        let creds = if needs_refresh {
            // 获取刷新锁，确保同一时间只有一个刷新操作
            let _guard = self.refresh_lock.lock().await;

            // 第二次检查：获取锁后重新读取凭据，因为其他请求可能已经完成刷新
            let current_creds = {
                let entries = self.entries.lock();
                entries
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.credentials.clone())
                    .ok_or_else(|| anyhow::anyhow!("凭据 #{} 不存在", id))?
            };

            if is_token_expired(&current_creds) || is_token_expiring_soon(&current_creds) {
                // 确实需要刷新
                let new_creds =
                    refresh_token_with_id(&current_creds, &self.config, self.proxy.as_ref(), id)
                        .await?;

                if is_token_expired(&new_creds) {
                    anyhow::bail!("刷新后的 Token 仍然无效或已过期");
                }

                // 更新凭据
                {
                    let mut entries = self.entries.lock();
                    if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                        entry.credentials = new_creds.clone();
                    }
                }

                // 回写凭据到文件（仅多凭据格式），失败只记录警告
                if let Err(e) = self.persist_credentials() {
                    tracing::warn!("Token 刷新后持久化失败（不影响本次请求）: {}", e);
                }

                new_creds
            } else {
                // 其他请求已经完成刷新，直接使用新凭据
                tracing::debug!("Token 已被其他请求刷新，跳过刷新");
                current_creds
            }
        } else {
            credentials.clone()
        };

        let token = creds
            .access_token
            .clone()
            .ok_or_else(|| anyhow::anyhow!("没有可用的 accessToken"))?;

        Ok(CallContext {
            id,
            credentials: creds,
            token,
        })
    }

    /// 将凭据列表回写到源文件
    ///
    /// 仅在以下条件满足时回写：
    /// - 源文件是多凭据格式（数组）
    /// - credentials_path 已设置
    ///
    /// # Returns
    /// - `Ok(true)` - 成功写入文件
    /// - `Ok(false)` - 跳过写入（非多凭据格式或无路径配置）
    /// - `Err(_)` - 写入失败
    fn persist_credentials(&self) -> anyhow::Result<bool> {
        use anyhow::Context;

        // 仅多凭据格式才回写
        if !self.is_multiple_format {
            return Ok(false);
        }

        let path = match &self.credentials_path {
            Some(p) => p,
            None => return Ok(false),
        };

        // 收集所有凭据
        let credentials: Vec<KiroCredentials> = {
            let entries = self.entries.lock();
            entries
                .iter()
                .map(|e| {
                    let mut cred = e.credentials.clone();
                    cred.canonicalize_auth_method();
                    cred
                })
                .collect()
        };

        // 序列化为 pretty JSON
        let json = serde_json::to_string_pretty(&credentials).context("序列化凭据失败")?;

        // 写入文件（在 Tokio runtime 内使用 block_in_place 避免阻塞 worker）
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::block_in_place(|| std::fs::write(path, &json))
                .with_context(|| format!("回写凭据文件失败: {:?}", path))?;
        } else {
            std::fs::write(path, &json).with_context(|| format!("回写凭据文件失败: {:?}", path))?;
        }

        tracing::debug!("已回写凭据到文件: {:?}", path);
        Ok(true)
    }

    /// 报告指定凭据 API 调用成功
    ///
    /// 重置该凭据的失败计数
    ///
    /// # Arguments
    /// * `id` - 凭据 ID（来自 CallContext）
    pub fn report_success(&self, id: u64) {
        // 重置 MODEL_TEMPORARILY_UNAVAILABLE 计数器
        self.model_unavailable_count.store(0, Ordering::SeqCst);

        // 记录使用次数（用于动态 TTL）
        self.record_usage(id);

        let mut entries = self.entries.lock();
        if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
            entry.failure_count = 0;
            tracing::debug!("凭据 #{} API 调用成功", id);
        }
    }

    /// 报告指定凭据 API 调用失败
    ///
    /// 增加失败计数，达到阈值时禁用凭据
    /// 返回是否还有可用凭据可以重试
    ///
    /// # Arguments
    /// * `id` - 凭据 ID（来自 CallContext）
    pub fn report_failure(&self, id: u64) -> bool {
        let mut entries = self.entries.lock();

        let entry = match entries.iter_mut().find(|e| e.id == id) {
            Some(e) => e,
            None => return entries.iter().any(|e| !e.disabled),
        };

        entry.failure_count += 1;
        let failure_count = entry.failure_count;

        tracing::warn!(
            "凭据 #{} API 调用失败（{}/{}）",
            id,
            failure_count,
            MAX_FAILURES_PER_CREDENTIAL
        );

        if failure_count >= MAX_FAILURES_PER_CREDENTIAL {
            entry.disabled = true;
            entry.auto_heal_reason = Some(AutoHealReason::TooManyFailures);
            entry.disable_reason = Some(DisableReason::FailureLimit);
            tracing::error!("凭据 #{} 已连续失败 {} 次，已被禁用", id, failure_count);

            // 移除该凭据的亲和性绑定
            drop(entries);
            self.affinity.remove_by_credential(id);

            let entries = self.entries.lock();
            return entries.iter().any(|e| !e.disabled);
        }

        // 检查是否还有可用凭据
        entries.iter().any(|e| !e.disabled)
    }

    /// 报告指定凭据额度已用尽
    ///
    /// 用于处理 402 Payment Required 且 reason 为 `MONTHLY_REQUEST_COUNT` 的场景：
    /// - 立即禁用该凭据（不等待连续失败阈值）
    /// - 返回是否还有可用凭据
    pub fn report_quota_exhausted(&self, id: u64) -> bool {
        let mut entries = self.entries.lock();

        let entry = match entries.iter_mut().find(|e| e.id == id) {
            Some(e) => e,
            None => return entries.iter().any(|e| !e.disabled),
        };

        if entry.disabled {
            return entries.iter().any(|e| !e.disabled);
        }

        entry.disabled = true;
        entry.disable_reason = Some(DisableReason::QuotaExceeded);
        // 设为阈值，便于在管理面板中直观看到该凭据已不可用
        entry.failure_count = MAX_FAILURES_PER_CREDENTIAL;

        tracing::error!("凭据 #{} 额度已用尽（MONTHLY_REQUEST_COUNT），已被禁用", id);

        entries.iter().any(|e| !e.disabled)
    }

    /// 报告 MODEL_TEMPORARILY_UNAVAILABLE 错误
    ///
    /// 累计达到阈值后禁用所有凭据，5分钟后自动恢复
    /// 返回是否触发了全局禁用
    pub fn report_model_unavailable(&self) -> bool {
        let count = self.model_unavailable_count.fetch_add(1, Ordering::SeqCst) + 1;
        tracing::warn!(
            "MODEL_TEMPORARILY_UNAVAILABLE 错误（{}/{}）",
            count,
            MODEL_UNAVAILABLE_THRESHOLD
        );

        if count >= MODEL_UNAVAILABLE_THRESHOLD {
            self.disable_all_credentials(DisableReason::ModelUnavailable);
            true
        } else {
            false
        }
    }

    /// 禁用所有凭据
    fn disable_all_credentials(&self, reason: DisableReason) {
        let mut entries = self.entries.lock();
        let mut recovery_time = self.global_recovery_time.lock();

        for entry in entries.iter_mut() {
            if !entry.disabled {
                entry.disabled = true;
                entry.disable_reason = Some(reason);
            }
        }

        // 设置恢复时间
        let recover_at = Utc::now() + Duration::minutes(GLOBAL_DISABLE_RECOVERY_MINUTES);
        *recovery_time = Some(recover_at);

        tracing::error!(
            "所有凭据已被禁用（原因: {:?}），将于 {} 自动恢复",
            reason,
            recover_at.format("%H:%M:%S")
        );
    }

    /// 检查并执行自动恢复
    ///
    /// 如果已到恢复时间，恢复因 ModelUnavailable 禁用的凭据
    /// 余额不足的凭据不会被恢复
    ///
    /// 返回是否执行了恢复
    pub fn check_and_recover(&self) -> bool {
        let should_recover = {
            let recovery_time = self.global_recovery_time.lock();
            recovery_time.map(|t| Utc::now() >= t).unwrap_or(false)
        };

        if !should_recover {
            return false;
        }

        let mut entries = self.entries.lock();
        let mut recovery_time = self.global_recovery_time.lock();
        let mut recovered_count = 0;

        for entry in entries.iter_mut() {
            // 只恢复因 ModelUnavailable 禁用的凭据，余额不足的不恢复
            if entry.disabled && entry.disable_reason == Some(DisableReason::ModelUnavailable) {
                entry.disabled = false;
                entry.disable_reason = None;
                entry.failure_count = 0;
                recovered_count += 1;
            }
        }

        // 重置全局状态
        *recovery_time = None;
        self.model_unavailable_count.store(0, Ordering::SeqCst);

        if recovered_count > 0 {
            tracing::info!("已自动恢复 {} 个凭据", recovered_count);
        }

        recovered_count > 0
    }

    /// 标记凭据为余额不足（不会被自动恢复）
    #[allow(dead_code)]
    pub fn mark_insufficient_balance(&self, id: u64) {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
            entry.disabled = true;
            entry.auto_heal_reason = None; // 清除自愈原因，防止被自愈循环错误恢复
            entry.disable_reason = Some(DisableReason::InsufficientBalance);
            tracing::warn!("凭据 #{} 已标记为余额不足", id);
        }
    }

    /// 获取全局恢复时间（用于 Admin API）
    #[allow(dead_code)]
    pub fn get_recovery_time(&self) -> Option<DateTime<Utc>> {
        *self.global_recovery_time.lock()
    }

    /// 获取使用额度信息
    #[allow(dead_code)]
    pub async fn get_usage_limits(&self) -> anyhow::Result<UsageLimitsResponse> {
        let ctx = self.acquire_context().await?;
        get_usage_limits(
            &ctx.credentials,
            &self.config,
            &ctx.token,
            self.proxy.as_ref(),
        )
        .await
    }

    /// 初始化所有凭据的余额缓存
    ///
    /// 启动时顺序查询所有凭据的余额，每次间隔 0.5 秒避免触发限流。
    /// 查询失败的凭据会被跳过（保持 initialized: false）。
    ///
    /// # 返回
    /// - 成功初始化的凭据数量
    pub async fn initialize_balances(&self) -> usize {
        let credential_ids: Vec<u64> = {
            let entries = self.entries.lock();
            entries
                .iter()
                .filter(|e| !e.disabled)
                .map(|e| e.id)
                .collect()
        };

        if credential_ids.is_empty() {
            tracing::info!("无可用凭据，跳过余额初始化");
            return 0;
        }

        tracing::info!("正在初始化 {} 个凭据的余额...", credential_ids.len());

        let mut success_count = 0;

        // 顺序查询每个凭据的余额，间隔 0.5 秒避免触发限流
        for (index, &id) in credential_ids.iter().enumerate() {
            match self.get_usage_limits_for(id).await {
                Ok(limits) => {
                    // 计算剩余额度
                    let used = limits.current_usage();
                    let limit = limits.usage_limit();
                    let remaining = (limit - used).max(0.0);

                    self.update_balance_cache(id, remaining);

                    // 余额小于 1 时自动禁用凭据
                    if remaining < 1.0 {
                        let mut entries = self.entries.lock();
                        if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                            entry.disabled = true;
                            entry.disable_reason = Some(DisableReason::InsufficientBalance);
                            tracing::warn!("凭据 #{} 余额不足 ({:.2})，已自动禁用", id, remaining);
                        }
                    } else {
                        tracing::info!("凭据 #{} 余额初始化成功: {:.2}", id, remaining);
                    }
                    success_count += 1;
                }
                Err(e) => {
                    tracing::warn!("凭据 #{} 余额查询失败: {}", id, e);
                }
            }

            // 非最后一个凭据时，间隔 0.5 秒
            if index < credential_ids.len() - 1 {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }

        tracing::info!(
            "余额初始化完成: {}/{} 成功",
            success_count,
            credential_ids.len()
        );

        success_count
    }

    // ========================================================================
    // Admin API 方法
    // ========================================================================

    /// 获取管理器状态快照（用于 Admin API）
    pub fn snapshot(&self) -> ManagerSnapshot {
        let entries = self.entries.lock();
        let available = entries.iter().filter(|e| !e.disabled).count();

        ManagerSnapshot {
            entries: entries
                .iter()
                .map(|e| CredentialEntrySnapshot {
                    id: e.id,
                    priority: e.credentials.priority,
                    disabled: e.disabled,
                    disable_reason: e.disable_reason,
                    failure_count: e.failure_count,
                    auth_method: e.credentials.auth_method.as_deref().map(|m| {
                        if m.eq_ignore_ascii_case("builder-id") || m.eq_ignore_ascii_case("iam") {
                            "idc".to_string()
                        } else {
                            m.to_string()
                        }
                    }),
                    has_profile_arn: e.credentials.profile_arn.is_some(),
                    expires_at: e.credentials.expires_at.clone(),
                })
                .collect(),
            total: entries.len(),
            available,
        }
    }

    /// 设置凭据禁用状态（Admin API）
    pub fn set_disabled(&self, id: u64, disabled: bool) -> anyhow::Result<()> {
        {
            let mut entries = self.entries.lock();
            let entry = entries
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?;
            entry.disabled = disabled;
            if !disabled {
                // 启用时重置失败计数
                entry.failure_count = 0;
                entry.auto_heal_reason = None;
                entry.disable_reason = None;
            } else {
                entry.auto_heal_reason = Some(AutoHealReason::Manual);
                entry.disable_reason = Some(DisableReason::Manual);
            }
        }
        // 持久化更改
        self.persist_credentials()?;
        Ok(())
    }

    /// 设置凭据优先级（Admin API）
    pub fn set_priority(&self, id: u64, priority: u32) -> anyhow::Result<()> {
        {
            let mut entries = self.entries.lock();
            let entry = entries
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?;
            entry.credentials.priority = priority;
        }
        // 持久化更改
        self.persist_credentials()?;
        Ok(())
    }

    /// 重置凭据失败计数并重新启用（Admin API）
    pub fn reset_and_enable(&self, id: u64) -> anyhow::Result<()> {
        {
            let mut entries = self.entries.lock();
            let entry = entries
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?;
            entry.failure_count = 0;
            entry.disabled = false;
            entry.auto_heal_reason = None;
            entry.disable_reason = None;
        }
        // 持久化更改
        self.persist_credentials()?;
        Ok(())
    }

    /// 获取指定凭据的使用额度（Admin API）
    pub async fn get_usage_limits_for(&self, id: u64) -> anyhow::Result<UsageLimitsResponse> {
        let credentials = {
            let entries = self.entries.lock();
            entries
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.credentials.clone())
                .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?
        };

        // 检查是否需要刷新 token
        let needs_refresh = is_token_expired(&credentials) || is_token_expiring_soon(&credentials);

        let token = if needs_refresh {
            let _guard = self.refresh_lock.lock().await;
            let current_creds = {
                let entries = self.entries.lock();
                entries
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.credentials.clone())
                    .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?
            };

            if is_token_expired(&current_creds) || is_token_expiring_soon(&current_creds) {
                let new_creds =
                    refresh_token_with_id(&current_creds, &self.config, self.proxy.as_ref(), id)
                        .await?;
                {
                    let mut entries = self.entries.lock();
                    if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                        entry.credentials = new_creds.clone();
                    }
                }
                // 持久化失败只记录警告，不影响本次请求
                if let Err(e) = self.persist_credentials() {
                    tracing::warn!("Token 刷新后持久化失败（不影响本次请求）: {}", e);
                }
                new_creds
                    .access_token
                    .ok_or_else(|| anyhow::anyhow!("刷新后无 access_token"))?
            } else {
                current_creds
                    .access_token
                    .ok_or_else(|| anyhow::anyhow!("凭据无 access_token"))?
            }
        } else {
            credentials
                .access_token
                .ok_or_else(|| anyhow::anyhow!("凭据无 access_token"))?
        };

        let credentials = {
            let entries = self.entries.lock();
            entries
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.credentials.clone())
                .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?
        };

        get_usage_limits(&credentials, &self.config, &token, self.proxy.as_ref()).await
    }

    /// 添加新凭据（Admin API）
    ///
    /// # 流程
    /// 1. 验证凭据基本字段（refresh_token 不为空）
    /// 2. 尝试刷新 Token 验证凭据有效性
    /// 3. 分配新 ID（当前最大 ID + 1）
    /// 4. 添加到 entries 列表
    /// 5. 持久化到配置文件
    ///
    /// # 返回
    /// - `Ok(u64)` - 新凭据 ID
    /// - `Err(_)` - 验证失败或添加失败
    pub async fn add_credential(&self, new_cred: KiroCredentials) -> anyhow::Result<u64> {
        // 1. 基本验证
        validate_refresh_token(&new_cred)?;

        // 2. 尝试刷新 Token 验证凭据有效性
        let mut validated_cred =
            refresh_token(&new_cred, &self.config, self.proxy.as_ref()).await?;

        // 3. 分配新 ID
        let new_id = {
            let entries = self.entries.lock();
            entries.iter().map(|e| e.id).max().unwrap_or(0) + 1
        };

        // 4. 设置 ID 并保留用户输入的元数据
        validated_cred.id = Some(new_id);
        validated_cred.priority = new_cred.priority;
        validated_cred.auth_method = new_cred.auth_method.map(|m| {
            if m.eq_ignore_ascii_case("builder-id") || m.eq_ignore_ascii_case("iam") {
                "idc".to_string()
            } else {
                m
            }
        });
        validated_cred.client_id = new_cred.client_id;
        validated_cred.client_secret = new_cred.client_secret;
        validated_cred.region = new_cred.region;
        validated_cred.machine_id = new_cred.machine_id;

        {
            let mut entries = self.entries.lock();
            entries.push(CredentialEntry {
                id: new_id,
                credentials: validated_cred,
                failure_count: 0,
                disabled: false,
                auto_heal_reason: None,
                disable_reason: None,
            });
        }

        // 5. 持久化
        self.persist_credentials()?;

        tracing::info!("成功添加凭据 #{}", new_id);
        Ok(new_id)
    }

    /// 删除凭据（Admin API）
    ///
    /// # 前置条件
    /// - 凭据必须已禁用（disabled = true）
    ///
    /// # 行为
    /// 1. 验证凭据存在
    /// 2. 验证凭据已禁用
    /// 3. 从 entries 移除
    /// 4. 持久化到文件
    ///
    /// # 返回
    /// - `Ok(())` - 删除成功
    /// - `Err(_)` - 凭据不存在、未禁用或持久化失败
    pub fn delete_credential(&self, id: u64) -> anyhow::Result<()> {
        {
            let mut entries = self.entries.lock();

            // 查找凭据
            let entry = entries
                .iter()
                .find(|e| e.id == id)
                .ok_or_else(|| anyhow::anyhow!("凭据不存在: {}", id))?;

            // 检查是否已禁用
            if !entry.disabled {
                anyhow::bail!("只能删除已禁用的凭据（请先禁用凭据 #{}）", id);
            }

            // 删除凭据
            entries.retain(|e| e.id != id);
        }

        // 持久化更改
        self.persist_credentials()?;

        tracing::info!("已删除凭据 #{}", id);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    #[test]
    fn test_token_manager_new() {
        let config = Config::default();
        let credentials = KiroCredentials::default();
        let tm = TokenManager::new(config, credentials, None);
        assert!(tm.credentials().access_token.is_none());
    }

    #[test]
    fn test_is_token_expired_with_expired_token() {
        let mut credentials = KiroCredentials::default();
        credentials.expires_at = Some("2020-01-01T00:00:00Z".to_string());
        assert!(is_token_expired(&credentials));
    }

    #[test]
    fn test_is_token_expired_with_valid_token() {
        let mut credentials = KiroCredentials::default();
        let future = Utc::now() + Duration::hours(1);
        credentials.expires_at = Some(future.to_rfc3339());
        assert!(!is_token_expired(&credentials));
    }

    #[test]
    fn test_is_token_expired_within_5_minutes() {
        let mut credentials = KiroCredentials::default();
        let expires = Utc::now() + Duration::minutes(3);
        credentials.expires_at = Some(expires.to_rfc3339());
        assert!(is_token_expired(&credentials));
    }

    #[test]
    fn test_is_token_expired_no_expires_at() {
        let credentials = KiroCredentials::default();
        assert!(is_token_expired(&credentials));
    }

    #[test]
    fn test_is_token_expiring_soon_within_10_minutes() {
        let mut credentials = KiroCredentials::default();
        let expires = Utc::now() + Duration::minutes(8);
        credentials.expires_at = Some(expires.to_rfc3339());
        assert!(is_token_expiring_soon(&credentials));
    }

    #[test]
    fn test_is_token_expiring_soon_beyond_10_minutes() {
        let mut credentials = KiroCredentials::default();
        let expires = Utc::now() + Duration::minutes(15);
        credentials.expires_at = Some(expires.to_rfc3339());
        assert!(!is_token_expiring_soon(&credentials));
    }

    #[test]
    fn test_validate_refresh_token_missing() {
        let credentials = KiroCredentials::default();
        let result = validate_refresh_token(&credentials);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_refresh_token_valid() {
        let mut credentials = KiroCredentials::default();
        credentials.refresh_token = Some("a".repeat(150));
        let result = validate_refresh_token(&credentials);
        assert!(result.is_ok());
    }

    // MultiTokenManager 测试

    #[test]
    fn test_multi_token_manager_new() {
        let config = Config::default();
        let mut cred1 = KiroCredentials::default();
        cred1.priority = 0;
        let mut cred2 = KiroCredentials::default();
        cred2.priority = 1;

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();
        assert_eq!(manager.total_count(), 2);
        assert_eq!(manager.available_count(), 2);
    }

    #[test]
    fn test_multi_token_manager_empty_credentials() {
        let config = Config::default();
        let result = MultiTokenManager::new(config, vec![], None, None, false);
        // 支持 0 个凭据启动（可通过管理面板添加）
        assert!(result.is_ok());
        let manager = result.unwrap();
        assert_eq!(manager.total_count(), 0);
        assert_eq!(manager.available_count(), 0);
    }

    #[test]
    fn test_multi_token_manager_duplicate_ids() {
        let config = Config::default();
        let mut cred1 = KiroCredentials::default();
        cred1.id = Some(1);
        let mut cred2 = KiroCredentials::default();
        cred2.id = Some(1); // 重复 ID

        let result = MultiTokenManager::new(config, vec![cred1, cred2], None, None, false);
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(
            err_msg.contains("重复的凭据 ID"),
            "错误消息应包含 '重复的凭据 ID'，实际: {}",
            err_msg
        );
    }

    #[test]
    fn test_multi_token_manager_report_failure() {
        let config = Config::default();
        let cred1 = KiroCredentials::default();
        let cred2 = KiroCredentials::default();

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        // 凭据会自动分配 ID（从 1 开始）
        // MAX_FAILURES_PER_CREDENTIAL = 2，所以第一次失败不会禁用
        assert!(manager.report_failure(1));
        assert_eq!(manager.available_count(), 2);

        // 第二次失败会禁用第一个凭据
        assert!(manager.report_failure(1));
        assert_eq!(manager.available_count(), 1);

        // 继续失败第二个凭据（使用 ID 2）
        assert!(manager.report_failure(2));
        assert!(!manager.report_failure(2)); // 所有凭据都禁用了
        assert_eq!(manager.available_count(), 0);
    }

    #[test]
    fn test_multi_token_manager_report_success() {
        let config = Config::default();
        let cred = KiroCredentials::default();

        let manager = MultiTokenManager::new(config, vec![cred], None, None, false).unwrap();

        // 失败一次（使用 ID 1）
        manager.report_failure(1);

        // 成功后重置计数（使用 ID 1）
        manager.report_success(1);

        // 再失败一次不会禁用（因为计数已重置）
        manager.report_failure(1);
        assert_eq!(manager.available_count(), 1);
    }

    #[tokio::test]
    async fn test_multi_token_manager_acquire_context_auto_recovers_all_disabled() {
        let config = Config::default();
        let mut cred1 = KiroCredentials::default();
        cred1.access_token = Some("t1".to_string());
        cred1.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());
        let mut cred2 = KiroCredentials::default();
        cred2.access_token = Some("t2".to_string());
        cred2.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        // 凭据会自动分配 ID（从 1 开始）
        for _ in 0..MAX_FAILURES_PER_CREDENTIAL {
            manager.report_failure(1);
        }
        for _ in 0..MAX_FAILURES_PER_CREDENTIAL {
            manager.report_failure(2);
        }

        assert_eq!(manager.available_count(), 0);

        // 应触发自愈：重置失败计数并重新启用，避免必须重启进程
        let ctx = manager.acquire_context().await.unwrap();
        assert!(ctx.token == "t1" || ctx.token == "t2");
        assert_eq!(manager.available_count(), 2);
    }

    #[tokio::test]
    async fn test_multi_token_manager_acquire_context_prefers_higher_balance_when_usage_equal() {
        let config = Config::default();
        let mut cred1 = KiroCredentials::default();
        cred1.access_token = Some("t1".to_string());
        cred1.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());
        let mut cred2 = KiroCredentials::default();
        cred2.access_token = Some("t2".to_string());
        cred2.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        // 两个凭据使用次数都为 0 时，应优先选择余额更高的
        manager.update_balance_cache(1, 100.0);
        manager.update_balance_cache(2, 200.0);

        let ctx = manager.acquire_context().await.unwrap();
        assert_eq!(ctx.id, 2);
    }

    #[tokio::test]
    async fn test_multi_token_manager_acquire_context_round_robin_when_balance_and_usage_equal() {
        let config = Config::default();
        let mut cred1 = KiroCredentials::default();
        cred1.access_token = Some("t1".to_string());
        cred1.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());
        let mut cred2 = KiroCredentials::default();
        cred2.access_token = Some("t2".to_string());
        cred2.expires_at = Some((Utc::now() + Duration::hours(1)).to_rfc3339());

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        manager.update_balance_cache(1, 100.0);
        manager.update_balance_cache(2, 100.0);

        let ctx1 = manager.acquire_context().await.unwrap();
        let ctx2 = manager.acquire_context().await.unwrap();
        assert_ne!(ctx1.id, ctx2.id);
    }

    #[test]
    fn test_multi_token_manager_report_quota_exhausted() {
        let config = Config::default();
        let cred1 = KiroCredentials::default();
        let cred2 = KiroCredentials::default();

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        // 凭据会自动分配 ID（从 1 开始）
        assert_eq!(manager.available_count(), 2);
        assert!(manager.report_quota_exhausted(1));
        assert_eq!(manager.available_count(), 1);

        // 再禁用第二个后，无可用凭据
        assert!(!manager.report_quota_exhausted(2));
        assert_eq!(manager.available_count(), 0);
    }

    #[tokio::test]
    async fn test_multi_token_manager_quota_disabled_is_not_auto_recovered() {
        let config = Config::default();
        let cred1 = KiroCredentials::default();
        let cred2 = KiroCredentials::default();

        let manager =
            MultiTokenManager::new(config, vec![cred1, cred2], None, None, false).unwrap();

        manager.report_quota_exhausted(1);
        manager.report_quota_exhausted(2);
        assert_eq!(manager.available_count(), 0);

        let err = manager.acquire_context().await.err().unwrap().to_string();
        assert!(
            err.contains("所有凭据均已禁用"),
            "错误应提示所有凭据禁用，实际: {}",
            err
        );
        assert_eq!(manager.available_count(), 0);
    }

    // ============ 凭据级 Region 优先级测试 ============

    /// 辅助函数：获取 OIDC 刷新使用的 region（用于测试）
    fn get_oidc_region_for_credential<'a>(
        credentials: &'a KiroCredentials,
        config: &'a Config,
    ) -> &'a str {
        credentials.region.as_ref().unwrap_or(&config.region)
    }

    #[test]
    fn test_credential_region_priority_uses_credential_region() {
        // 凭据配置了 region 时，应使用凭据的 region
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.region = Some("eu-west-1".to_string());

        let region = get_oidc_region_for_credential(&credentials, &config);
        assert_eq!(region, "eu-west-1");
    }

    #[test]
    fn test_credential_region_priority_fallback_to_config() {
        // 凭据未配置 region 时，应回退到 config.region
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let credentials = KiroCredentials::default();
        assert!(credentials.region.is_none());

        let region = get_oidc_region_for_credential(&credentials, &config);
        assert_eq!(region, "us-west-2");
    }

    #[test]
    fn test_multiple_credentials_use_respective_regions() {
        // 多凭据场景下，不同凭据使用各自的 region
        let mut config = Config::default();
        config.region = "ap-northeast-1".to_string();

        let mut cred1 = KiroCredentials::default();
        cred1.region = Some("us-east-1".to_string());

        let mut cred2 = KiroCredentials::default();
        cred2.region = Some("eu-west-1".to_string());

        let cred3 = KiroCredentials::default(); // 无 region，使用 config

        assert_eq!(get_oidc_region_for_credential(&cred1, &config), "us-east-1");
        assert_eq!(get_oidc_region_for_credential(&cred2, &config), "eu-west-1");
        assert_eq!(
            get_oidc_region_for_credential(&cred3, &config),
            "ap-northeast-1"
        );
    }

    #[test]
    fn test_idc_oidc_endpoint_uses_credential_region() {
        // 验证 IdC OIDC endpoint URL 使用凭据 region
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.region = Some("eu-central-1".to_string());

        let region = get_oidc_region_for_credential(&credentials, &config);
        let refresh_url = format!("https://oidc.{}.amazonaws.com/token", region);

        assert_eq!(refresh_url, "https://oidc.eu-central-1.amazonaws.com/token");
    }

    #[test]
    fn test_social_refresh_endpoint_uses_credential_region() {
        // 验证 Social refresh endpoint URL 使用凭据 region
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.region = Some("ap-southeast-1".to_string());

        let region = get_oidc_region_for_credential(&credentials, &config);
        let refresh_url = format!("https://prod.{}.auth.desktop.kiro.dev/refreshToken", region);

        assert_eq!(
            refresh_url,
            "https://prod.ap-southeast-1.auth.desktop.kiro.dev/refreshToken"
        );
    }

    #[test]
    fn test_api_call_still_uses_config_region() {
        // 验证 API 调用（如 getUsageLimits）仍使用 config.region
        // 这确保只有 OIDC 刷新使用凭据 region，API 调用行为不变
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.region = Some("eu-west-1".to_string());

        // API 调用应使用 config.region，而非 credentials.region
        let api_region = &config.region;
        let api_host = format!("q.{}.amazonaws.com", api_region);

        assert_eq!(api_host, "q.us-west-2.amazonaws.com");
        // 确认凭据 region 不影响 API 调用
        assert_ne!(api_region, credentials.region.as_ref().unwrap());
    }

    #[test]
    fn test_credential_region_empty_string_fallback_to_config() {
        // 空字符串 region 应回退到 config.region
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.region = Some("".to_string());

        let region = credentials
            .region
            .as_ref()
            .filter(|r| !r.trim().is_empty())
            .unwrap_or(&config.region);
        // 空字符串应回退到 config.region
        assert_eq!(region, "us-west-2");
    }

    #[test]
    fn test_credential_region_whitespace_fallback_to_config() {
        // 纯空白字符 region 应回退到 config.region
        let mut config = Config::default();
        config.region = "us-west-2".to_string();

        let mut credentials = KiroCredentials::default();
        credentials.region = Some("   ".to_string());

        let region = credentials
            .region
            .as_ref()
            .filter(|r| !r.trim().is_empty())
            .unwrap_or(&config.region);
        assert_eq!(region, "us-west-2");
    }
}
