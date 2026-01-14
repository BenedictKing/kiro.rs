# Changelog

## [Unreleased]

### Changed
- 调整全局禁用恢复时间：`GLOBAL_DISABLE_RECOVERY_MINUTES` 从 10 分钟降至 5 分钟
  - 加快模型暂时不可用后的自动恢复速度
- 调整总重试次数硬上限：`MAX_TOTAL_RETRIES` 从 5 降至 3
  - 进一步减少无效重试开销，加快故障转移速度
- 余额初始化改为顺序查询，每次间隔 0.5 秒避免触发限流
  - 从并发查询改为顺序查询（`initialize_balances()`）
  - 移除 30 秒整体超时机制
  - 涉及文件：`src/kiro/token_manager.rs`

### Fixed
- 修复 `MODEL_TEMPORARILY_UNAVAILABLE` 错误检测逻辑未实际调用的问题
  - 在 `call_mcp()` 和 `call_api()` 中添加错误检测和熔断触发逻辑
  - 移除 `report_model_unavailable()` 和 `disable_all_credentials()` 的 `#[allow(dead_code)]` 标记
  - 现在当检测到该错误时会正确触发全局熔断机制

### Added
- 新增 WebSearch 工具支持（`src/anthropic/websearch.rs`）
  - 实现 Anthropic WebSearch 请求到 Kiro MCP 的转换
  - 支持 SSE 流式响应，生成完整的搜索结果事件序列
  - 自动检测纯 WebSearch 请求（tools 仅包含 web_search）并路由到专用处理器
- 新增 MCP API 调用支持（`src/kiro/provider.rs`）
  - 新增 `call_mcp()` 方法，支持 WebSearch 等工具调用
  - 新增 `mcp_url()` 和 `build_mcp_headers()` 方法
  - 完整的重试和故障转移逻辑
- 新增凭据级 `region` 字段，用于 OIDC token 刷新时指定 endpoint 区域
  - 未配置时回退到 config.json 的全局 region
  - API 调用仍使用 config.json 的 region
- 新增凭据级 `machineId` 字段，支持每个凭据使用独立的机器码
  - 支持 64 字符十六进制和 UUID 格式（自动标准化）
  - 未配置时回退到 config.json 的 machineId，都未配置时由 refreshToken 派生
  - 启动时自动补全并持久化到配置文件
- 新增 GitHub Actions Docker 构建工作流（`.github/workflows/docker-build.yaml`）
  - 支持 linux/amd64 和 linux/arm64 双架构
  - 推送到 GitHub Container Registry

### Changed
- 版本号升级至 2026.1.5
- TLS 库从 native-tls 切换至 rustls（reqwest 依赖调整）
- `authMethod` 自动推断：未指定时根据是否有 clientId/clientSecret 自动判断为 idc 或 social
- 移除 web_search/websearch 工具过滤（`is_unsupported_tool` 现在返回 false）

### Fixed
- 修复 machineId 格式兼容性问题，支持 UUID 格式自动转换为 64 字符十六进制

### Removed
- 移除 `current_id` 概念（后端和前端）
  - 后端：移除 `MultiTokenManager.current_id` 字段和相关方法（`switch_to_next`、`select_highest_priority`、`select_by_balance`、`credentials`）
  - 后端：移除 `ManagerSnapshot.current_id` 字段
  - 后端：移除 `CredentialStatusItem.is_current` 字段
  - 前端：移除 `CredentialsStatusResponse.currentId` 和 `CredentialStatusItem.isCurrent`
  - 原因：多用户并发访问时，"当前凭据"概念无意义，凭据选择由 `acquire_context_for_user()` 动态决定

### Added
- 新增启动时余额初始化功能
  - `initialize_balances()`: 启动时并发查询所有凭据余额并更新缓存
  - 整体超时 30 秒，避免阻塞启动流程
  - 初始化失败或超时时输出警告日志

### Changed
- 改进凭据选择算法：从单一"使用次数最少"改为两级排序
  - 第一优先级：使用次数最少
  - 第二优先级：余额最多（使用次数相同时）
  - 未初始化余额的凭据会被降级处理，避免被优先选中
- 移除前端"当前活跃"凭据展示
  - 前端：移除凭据卡片的"当前"高亮和 Badge
  - 前端：移除 Dashboard 中的"当前活跃"统计卡片
  - 统计卡片布局从 3 列调整为 2 列

### Added
- 新增 `sensitive-logs` feature flag，显式启用才允许打印潜在敏感信息（仅用于排障）
  - 默认关闭：Kiro 请求体只输出长度，凭证只输出摘要信息
  - 启用方式：`cargo build --features sensitive-logs`

### Fixed
- 修复 SSE 流 ping 保活首次立即触发的问题
  - 使用 `interval_at(Instant::now() + ping_period, ping_period)` 延迟首次触发
  - 避免连接建立后立即发送无意义的 ping 事件

### Changed
- 改进服务启动错误处理
  - 绑定监听地址失败时输出错误日志并退出（exit code 1）
  - HTTP 服务异常退出时输出错误日志并退出（exit code 1）

### Fixed
- 修复合并上游后 `CredentialEntry` 结构体字段缺失导致的编译错误
  - 添加 `disable_reason: Option<DisableReason>` 字段（公共 API 展示用）
  - 添加 `auto_heal_reason: Option<AutoHealReason>` 字段（内部自愈逻辑用）
- 修复禁用原因字段不同步问题
  - `report_failure()`: 禁用时同步设置两个字段
  - `set_disabled()`: 启用/禁用时同步设置/清除两个字段
  - `reset_and_enable()`: 重置时同步清除两个字段
  - 自愈循环：重新启用凭据时同步清除 `disable_reason`
  - `mark_insufficient_balance()`: 清除 `auto_heal_reason` 防止被自愈循环错误恢复

### Changed
- 重命名内部字段以提高可读性
  - `DisabledReason` → `AutoHealReason`（自愈原因枚举）
  - `disabled_reason` → `auto_heal_reason`（自愈原因字段）
- 日志中的 `user_id` 现在会进行掩码处理，保护用户隐私
  - 长度 > 25：保留前13后8字符（如 `user_f516339a***897ac7`）
  - 长度 13-25：保留前4后4字符
  - 长度 ≤ 12：完全掩码为 `***`

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
- 修复关闭余额弹窗后卡片不更新缓存余额的问题
  - 弹窗关闭时调用 `queryClient.invalidateQueries({ queryKey: ['cached-balances'] })`
  - 确保卡片和弹窗使用的两个独立数据源保持同步

### Changed
- 增强 Token 刷新日志，添加凭证 ID 追踪
  - 新增 `refresh_token_with_id()` 函数支持传入凭证 ID
  - 日志现在包含 `credential_id` 字段，便于多凭据环境下的问题排查

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
  - 5 分钟后自动恢复（余额不足的凭据除外）
- `CredentialEntrySnapshot` 新增 `disable_reason` 字段，支持查询禁用原因
- 新增自动余额刷新：成功请求后自动在后台刷新余额缓存（基于动态 TTL 策略）
  - 新增 `spawn_balance_refresh()` 方法，使用 `tokio::spawn` 异步刷新
  - 新增 `should_refresh_balance()` 方法，根据 TTL 判断是否需要刷新
