# Changelog

## [Unreleased]

### Added
- 新增缓存余额查询 API（`GET /credentials/balances/cached`）
  - 后端：`CachedBalanceInfo` 结构体、`get_all_cached_balances()` 方法
  - 前端：凭据卡片直接显示缓存余额和更新时间
  - 30 秒自动轮询更新，缓存超过 1 分钟时点击强制刷新
- 新增 Bonus 用量包支持（`src/kiro/model/usage_limits.rs`）
  - 新增 `Bonus` 结构体，支持 GIFT 类型的额外用量包
  - 新增 `Bonus::is_active()` 方法，按状态/过期时间判断是否激活
  - `usage_limit()` 和 `current_usage()` 现在会合并基础额度、免费试用额度和所有激活的 bonuses
- 新增 Kiro Web Portal API 模块（`src/kiro/web_portal.rs`）
  - 支持 CBOR 协议与 app.kiro.dev 通信
  - 实现 `get_user_info()` 和 `get_user_usage_and_limits()` API
  - 新增 `aggregate_account_info()` 聚合账号信息（套餐/用量/邮箱等）
- Admin UI 前端增强
  - 新增数字格式化工具（`admin-ui/src/lib/format.ts`）：K/M/B 显示、Token 对格式化、过期时间格式化
  - 新增统计相关 API 和 Hooks：`getCredentialStats`, `resetCredentialStats`, `resetAllStats`
  - 新增账号信息 API：`getCredentialAccountInfo`, `useCredentialAccountInfo`
  - 扩展 `CredentialStatusItem` 添加统计字段（调用次数、Token 用量、最后调用时间等）
  - 新增完整的账号信息类型定义（`AccountAggregateInfo`, `CreditsUsageSummary` 等）
- 新增 `serde_cbor` 依赖用于 CBOR 编解码

### Fixed
- 修复手动查询余额后列表页面不显示缓存余额的问题
  - `get_balance()` 成功后调用 `update_balance_cache()` 更新缓存
  - 现在点击"查看余额"后，列表页面会正确显示缓存的余额值

### Changed
- 调整重试策略：单凭据最大重试次数 3→2，单请求最大重试次数 9→5
  - `MAX_RETRIES_PER_CREDENTIAL`: 3 → 2
  - `MAX_TOTAL_RETRIES`: 9 → 5
  - `MAX_FAILURES_PER_CREDENTIAL`: 3 → 2
  - 减少无效凭据的重试开销，加快故障转移速度

### Added
- 新增用户亲和性绑定功能：连续对话优先使用同一凭据（基于 `metadata.user_id`）
  - 新增 `src/kiro/affinity.rs` 模块，实现 `UserAffinityManager`
  - 新增 `acquire_context_for_user()` 方法支持亲和性查询
  - 亲和性绑定 TTL 为 30 分钟
- 新增余额感知故障转移：凭据失效时自动切换到余额最高的可用凭据
- 新增动态余额缓存 TTL 策略：
  - 高频渠道（10分钟内 ≥20 次调用）：10 分钟刷新
  - 低频渠道：30 分钟刷新
  - 低余额渠道（余额 < 1.0）：24 小时刷新
- 新增 `record_usage()` 方法自动记录凭据使用频率
- 新增负载均衡：无亲和性绑定时优先分配到使用频率最低的凭据
- 新增 `DisableReason` 枚举，区分凭据禁用原因（失败次数、余额不足、模型不可用、手动禁用）
- 成功请求时自动重置 `MODEL_TEMPORARILY_UNAVAILABLE` 计数器，避免跨时间累计触发
- 新增 `MODEL_TEMPORARILY_UNAVAILABLE` 错误检测和全局禁用机制
  - 当该 500 错误发生 2 次时，自动禁用所有凭据
  - 10 分钟后自动恢复（余额不足的凭据除外）
- `CredentialEntrySnapshot` 新增 `disable_reason` 字段，支持查询禁用原因
- 新增自动余额刷新：成功请求后自动在后台刷新余额缓存（基于动态 TTL 策略）
  - 新增 `spawn_balance_refresh()` 方法，使用 `tokio::spawn` 异步刷新
  - 新增 `should_refresh_balance()` 方法，根据 TTL 判断是否需要刷新
