# kiro.rs 新模型支持与监控体系升级计划

> 日期：2026-07-02
> 来源：跨 20+ 同类项目的特性调研（kiro.rs-upstream / kirocc / kiro.rs-ZyphrZero / kiro-go / kiro-sub 等）

## 0. 目标概述

四层递进升级：

1. **P0 模型层**：支持 Sonnet 5 / Fable 5 / Opus 4.8 三个新模型，引入 5 级 effort 系统，修正响应模型 ID
2. **P1 数据层**：SQLite 请求日志（attempt 级粒度）、统计增强（今日/累计/tokens）、长冷却持久化、新 Admin API
3. **P2 展示层**：Admin UI 改为 4 Tab 列表布局，新增请求日志与统计概览页面
4. **P3 健壮性**：从 upstream 移植 Converter 增强（tool 配对校验、消息合并等）

**关键决策**（调研结论）：
- 存储**不引入 Redis/MySQL**——数据量（7 凭据 ~80KB）远未到量级，运维成本与故障点得不偿失；改用 JSON 文件 + SQLite（仅请求日志）
- kiro-go 前端日志**不照做**（单文件 2511 行 vanilla JS 不可维护），仅借鉴其 attempt 级数据结构，在现有 React 19 + shadcn 架构上原创实现
- SQLite 日志从展示层提前到数据层——统计 Tab 的 byDay/byModel 聚合只能由它提供

## 1. P0 — 模型层（独立无依赖）

### 1.1 新模型映射

**文件**：`src/anthropic/converter.rs`（现有映射在 148-196 行）

新增常量与映射规则：

| Anthropic 输入 | Kiro SKU | 上下文 | 依据 |
|---|---|---|---|
| `claude-opus-4-8` / `4.8` | `claude-opus-4.8` | 1M（always） | upstream 已验证（commit b9e757e） |
| `claude-sonnet-5` | `claude-sonnet-5` | 1M（always，无 200K SKU） | kirocc v0.5.0 已验证 |
| `claude-fable-5` | `claude-fable-5` | 1M（预设） | **直通预留**，无项目验证过真实 SKU |
| `claude-sonnet-4-8` / `4.8` | `claude-sonnet-4.8` | 1M | ZyphrZero 已验证（可选） |

实现要点：
- 沿用现有 `normalize_model_name()`（剥离 `-thinking` / `-agentic` 后缀）
- `[1m]` 后缀在映射前剥离（kirocc 两级查找模式：先精确匹配，再剥后缀匹配）
- opus 分支的匹配顺序：`4-5 → 4-7 → 4-8 → 默认 4-6`，注意 `contains` 匹配不能让 `4-8` 落入默认分支
- fable 是新模型族，在 sonnet/opus/haiku 之外新增 `contains("fable")` 分支
- **Fable 5 失败路径**：直通 SKU 若上游拒绝，捕获上游 400 中的模型相关错误，返回明确的 `unsupported model` 错误信息而非模糊 502

### 1.2 /v1/models 与 count_tokens 端点

**文件**：`src/anthropic/handlers.rs`（`get_models` 在 628-859 行）

- 为每个新模型添加 base / `-thinking` / `-agentic` 三变体条目（沿用现有 18 条的结构）
- 新条目参数：`context_length: 1_000_000`，Opus 4.8 `max_completion_tokens: 128_000`，Sonnet 5 / Fable 5 `max_completion_tokens: 64_000`
- 新增 `[1m]` 后缀别名条目（`claude-opus-4-8[1m]`、`claude-sonnet-5[1m]` 等），匹配 Claude Code Max 计划默认发送的模型名
- `count_tokens` 端点确认能识别新模型名（走同一 `map_model`，需回归验证）

### 1.3 响应模型 ID 修正（[1m] 后缀）

**来源**：kirocc `respconv/` 的核心洞察——Claude Code 客户端 `mR()` 函数根据响应 `model` 字段判断 1M 上下文。

规则（kirocc e2e 测试已验证）：

```
请求 claude-sonnet-5      → 上游 claude-sonnet-5 → 响应 claude-sonnet-5[1m]
请求 claude-sonnet-5[1m]  → 上游 claude-sonnet-5 → 响应 claude-sonnet-5[1m]（原样保留）
请求 claude-opus-4-8      → 上游 claude-opus-4.8 → 响应 claude-opus-4-8[1m]
```

- always-1M 模型（Opus 4.6/4.7/4.8、Sonnet 5、Fable 5）响应中自动补 `[1m]` 后缀
- 带 `[1m]` 请求的原样保留（不重复追加）
- `[1m]` 后缀**不隐含开启 thinking**——thinking 仍由 `thinking` 字段 / 模型名 `-thinking` 后缀显式控制

**改动点（两处）**：
- `src/anthropic/stream.rs` — 流式 `message_start` 事件的 `model` 字段
- `src/anthropic/handlers.rs` — 非流式响应组装处

### 1.4 Effort 层级系统

**权威来源**：kirocc `internal/models/effort.go`（数据来自 kiro-cli 2.10.0 `ListAvailableModels` 实际 schema，比 ZyphrZero 的"仅 opus-4.6 + adaptive 才发送"约束更新更可信）

```rust
// 5 级枚举，低→高
enum EffortTier { Low, Medium, High, XHigh, Max }

// 每模型允许的 effort 集合
// 5 值（含 xhigh）: claude-opus-4.8, claude-opus-4.7, claude-sonnet-5, claude-fable-5
// 4 值（无 xhigh）: claude-opus-4.6, claude-sonnet-4.6（及 -1m 变体）
// 其余模型: 不支持 effort，完全省略 additionalModelRequestFields
```

解析与降级规则（kirocc `ResolveEffort` 语义）：
- 无效字符串（拼写错误、`"enabled"` 等）→ 丢弃不发送，绝不猜测
- 合法但模型不支持的级别 → 映射到该模型最高支持级（实际只有 `xhigh` 在 4 值模型上降为 `max`）
- 模型完全不支持 effort → 静默省略字段
- 未知新模型（如 `claude-sonnet-5.1`）→ 采用 ZyphrZero 乐观策略，默认放行 `xhigh`（面向未来）

发送位置：Kiro 请求体 `additionalModelRequestFields.output_config.effort`。effort 来源：请求体 `output_config.effort` 字段透传 + 可扩展模型名后缀（如 `-effort-xhigh`，本期可选）。

## 2. P1 — 数据层（UI 的前置依赖）

### 2.1 请求日志：TraceSink + SQLite

**参考实现**：ZyphrZero `admin/trace_db.rs`（SQLite WAL 模式已验证）+ kiro-go `handler.go` 的 attempt 级数据结构。

**新文件**：`src/kiro/trace.rs`（TraceSink trait + 类型）、`src/admin/trace_db.rs`（SQLite 存储）

```sql
CREATE TABLE request_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL,               -- ISO8601
    ts_epoch INTEGER NOT NULL,      -- 索引用
    path TEXT NOT NULL,             -- /v1/messages
    model TEXT,                     -- 请求的 Anthropic 模型名
    credential_id INTEGER,          -- 最终使用的凭据
    email_masked TEXT,              -- 写入时即脱敏 us***@ex***.com
    is_stream INTEGER,
    attempts INTEGER,
    final_status INTEGER,           -- 最终 HTTP 状态
    duration_ms INTEGER,
    first_token_ms INTEGER,         -- 流式首 token 延迟
    input_tokens INTEGER, output_tokens INTEGER,
    error TEXT                      -- 截断 300 字符
);
CREATE INDEX idx_logs_ts ON request_logs(ts_epoch);

CREATE TABLE request_attempts (
    log_id INTEGER, try_number INTEGER,
    credential_id INTEGER, email_masked TEXT,
    status_code INTEGER, outcome TEXT,   -- success/throttled/auth_failed/transient/...
    duration_ms INTEGER, error TEXT,     -- 截断 300 字符
    PRIMARY KEY (log_id, try_number)
);
```

**写入架构（关键设计，避免热路径阻塞）**：
- `TraceSink` trait：provider 每次重试调用 `on_attempt(TraceAttempt)`，请求结束调用 `finish(TraceRecord)`
- 日志经 `tokio::sync::mpsc` channel 发送到**单一后台写任务**，热路径零同步 IO
- channel 满时丢弃日志并计数（日志系统绝不能拖垮代理主功能）

**流式响应的写入时机**（难点，必须覆盖三种结束方式）：
1. 正常结束（`message_stop`）→ 记录完整 usage
2. 客户端提前断开 → 在 stream Drop guard 中 flush，`final_status` 标记为已发送的状态、error 标 `client_disconnected`
3. 上游中断 → 记录 `stream_interrupted` + 已传输字节数

**保留策略**：默认 7 天，后台任务每日清理（`DELETE WHERE ts_epoch < cutoff`，attempts 先删）。库文件放凭据同目录 `request_logs.db`，WAL 模式 + `synchronous=NORMAL`。

**依赖**：`rusqlite`（bundled feature，免系统依赖）。不做 feature flag——日志是本次升级的核心功能，默认启用，提供配置项 `requestLogRetentionDays`（0 = 关闭）。

### 2.2 StatsEntry 增强

**文件**：`src/kiro/token_manager.rs`（StatsEntry 在 572-576 行，load/save 在 1952-2047 行）

```rust
#[derive(Serialize, Deserialize)]
struct StatsEntry {
    success_count: u64,                    // 已有
    #[serde(default)] failure_count: u64,  // 新增：累计最终失败（ZyphrZero 同款）
    #[serde(default)] attempt_failure_count: u64, // 新增：含重试的尝试失败
    #[serde(default)] daily_count: u32,    // 新增：今日请求量
    #[serde(default)] daily_date: String,  // 新增：本地时区日历日 "2026-07-02"
    #[serde(default)] total_input_tokens: u64,   // 新增
    #[serde(default)] total_output_tokens: u64,  // 新增
    last_used_at: Option<String>,          // 已有
}
```

要点：
- **所有新字段带 `#[serde(default)]`**——老 kiro_stats.json 平滑升级，缺字段按 0 处理
- 今日计数按**本地时区**日历日重置：每次记录时比较 `daily_date != today_local()` 则清零重开（用户直觉优先；上游配额虽按 UTC 重置，但余额有独立 balance 查询）
- 双失败口径（kiro-go 模式）：`failure_count` = 重试耗尽的最终失败（用户视角）；`attempt_failure_count` = 每次尝试失败（凭据健康视角）
- token 用量从 `stream.rs` 的 StreamContext（本地估算口径）回传至 `record_api_success`
- 全局统计不单独建文件——由各凭据 StatsEntry 求和 + SQLite 日志聚合得出
- 沿用现有 30 秒 debounce + 原子写机制

### 2.3 长冷却持久化

**文件**：`src/kiro/cooldown.rs`（CooldownEntry 在 82-94 行）

**问题**：QuotaExhausted / AccountSuspended 的 24h 冷却纯内存，重启后立即重打已耗尽配额的账号。

方案：
- `CooldownEntry` 增加序列化能力，`expires_at` 从 `Instant` 改为 `DateTime<Utc>`（Instant 不可序列化且跨进程无意义）
- 仅持久化**非自动恢复类**（QuotaExhausted / AccountSuspended / AuthenticationFailed）到 `kiro_cooldowns.json`（凭据同目录）
- 短冷却（RateLimit 60s / ServerError 120s 等）保持纯内存——丢失无害
- 启动时加载，已过期的条目直接丢弃
- 写入时机：`set_cooldown` 遇到需持久化的 reason 时立即原子写（低频事件，无需 debounce）

### 2.4 新增 Admin API

**文件**：`src/admin/router.rs` + `src/admin/handlers.rs` + `src/admin/service.rs`

| 端点 | 返回 | 数据源 |
|---|---|---|
| `GET /admin/api/stats` | 全局统计：总/成功/失败/重试数、今日请求、总 tokens、uptime | StatsEntry 求和 + 进程启动时间 |
| `GET /admin/api/cooldowns` | 全部凭据冷却状态：reason、started_at、expires_at、`remaining_secs` 倒计时、trigger_count | CooldownManager 新增 `get_all_statuses()` |
| `GET /admin/api/request-logs?limit=100&status=&credential_id=&before=` | 日志列表（倒序分页）+ 每条的 attempts 明细 | SQLite |
| `GET /admin/api/credentials/:id/stats` | 单凭据：今日/累计/失败/tokens/byDay(近30天)/byModel | StatsEntry + SQLite `GROUP BY date(ts)` / `GROUP BY model` |

- 现有 `CredentialStatsResponse` 前端类型已定义 `byDay` / `byModel` 数组（空架子），本期由 SQLite 聚合真正产出
- 所有端点走现有 admin 认证中间件（adminApiKey）

## 3. P2 — 展示层（消费 P1 数据） ✅ 已完成

### 3.1 4 Tab 布局重构

**现状**：`admin-ui/src/components/dashboard.tsx`（838 行单页）无 Tab 无路由。

**方案**：引入 shadcn `Tabs` 组件（保持无路由的轻量 SPA，不引入 react-router）：

| Tab | 内容 | 工作量 |
|---|---|---|
| 凭据管理 | 现有卡片网格 + 批量操作 + 状态增强（见 3.4） | 重构移动 |
| 请求日志 | 新建（见 3.2） | 新建 |
| 统计概览 | 新建（见 3.3） | 新建 |
| 系统配置 | 现有 global-config / proxy-config 对话框改为内嵌面板 | 重构移动 |

- `dashboard.tsx` 拆分为 `tabs/credentials-tab.tsx`、`tabs/logs-tab.tsx`、`tabs/stats-tab.tsx`、`tabs/settings-tab.tsx`
- 顶部保留全局统计条（总凭据/可用凭据/今日请求/全局冷却告警），所有 Tab 共享
- 借鉴 kiro-go：可选"隐私模式"开关（邮箱前端二次脱敏显示）

### 3.2 请求日志 Tab

**原创设计**（借鉴 kiro-go 数据结构，React 实现）：

```
┌─ /v1/messages ── [200] ── 1.2s ── claude-sonnet-5 ─────────────┐
│ cred #3 us***@ex***.com   3 次尝试   07-02 14:23:45   ↑1.2k ↓4.5k │
│ ▼ 重试详情                                                        │
│   #1  [429]  cred #1  230ms  rate limit exceeded                 │
│   #2  [429]  cred #2  180ms  rate limit exceeded                 │
│   #3  [200]  cred #3  790ms                                      │
└───────────────────────────────────────────────────────────────────┘
```

- shadcn `Collapsible` 实现展开/收起 attempt 明细
- 状态码颜色：2xx 绿 / 4xx 黄 / 429 橙 / 5xx 红
- 筛选：状态码、凭据、模型；游标分页（`before` 参数）
- TanStack Query `refetchInterval: 10000` 自动刷新（比 kiro-go 手动刷新体验好）
- 空态与加载骨架（复用现有 `Skeleton` 组件）

### 3.3 统计概览 Tab

数据源：`GET /admin/api/stats` + `GET /admin/api/credentials/:id/stats`（byDay/byModel）

- 顶部指标卡：总请求 / 成功率 / 今日请求 / 总 tokens / uptime
- **近 30 天请求趋势图**（byDay 数据）——轻量方案用纯 CSS/SVG 柱状图，或引入 `recharts`（kiro-account-manager 同款，评估打包体积后定）
- **按模型分布**（byModel 数据）——横向条形图
- 凭据维度表格：每凭据的今日/累计/失败率/tokens，可排序

### 3.4 凭据卡片状态增强

**文件**：`admin-ui/src/components/credential-card.tsx`

新增显示项：
- **冷却状态徽章**：`⚠️ 冷却中: RateLimitExceeded（剩余 45s）`——前端本地倒计时（拿到 `remaining_secs` 后 setInterval 递减，避免频繁轮询），到 0 自动消失并触发 refetch
- **今日请求数 / 累计请求数**：来自增强后的 StatsEntry
- **失败率**：`failure_count / (success_count + failure_count)`，>10% 黄色、>30% 红色
- cooldown reason 的中文映射：`RateLimitExceeded → 限流冷却`、`QuotaExhausted → 配额耗尽`、`AccountSuspended → 账号暂停` 等

## 4. P3 — 健壮性（Converter 增强） ✅ 已存在（无需额外实现）

从 kiro.rs-upstream 移植（均已在上游验证）：

| 特性 | upstream 位置 | 说明 |
|---|---|---|
| tool_use/tool_result 配对校验 | `converter.rs:432-513` `validate_tool_pairing()` | 过滤孤儿 tool_result，检测孤儿 tool_use |
| 孤儿 tool_use 清理 | `converter.rs:516-549` `remove_orphaned_tool_uses()` | 从历史消息移除，防上游 400 |
| 连续 assistant 消息合并 | `converter.rs:859-895` `merge_assistant_messages()` | 处理网络不稳定产生的连续消息（upstream Issue #79） |
| Write/Edit 分块写入策略注入 | `converter.rs:66-76, 591-599` | 向 Write/Edit 工具描述注入 50 行分块策略，缓解上游输出截断 |
| Thinking 标签引号检测 | `stream.rs:37-172` | 区分真实 `<thinking>` 标签与引号/反引号包裹的引用，减少误判 |

注意：移植前逐一对比当前 kiro.rs 是否已有等价实现（当前已有部分 tool 配对修复逻辑在 compressor.rs），**避免重复实现**——若已有，以对比后较完善的版本为准。

可选补充（本期不做，记录备查）：
- 慢模型自适应 timeout（KiroGate_v：Opus 类超时 ×2）
- 图片 SHA256 去重（ZyphrZero：重复图片替换为占位文本）
- KIRO_API_KEY 环境变量凭据（upstream：`ksk_` 格式 headless Bearer）

## 5. 明确不做的事项

| 事项 | 原因 |
|---|---|
| Redis / MySQL | 数据量级不匹配（~80KB），运维成本与故障点高于收益；SQLite 覆盖日志场景 |
| 照搬 kiro-go 前端 | 单文件 2511 行 vanilla JS 不可维护；现有 React 架构更优，仅借鉴数据结构 |
| react-router 多页路由 | Tabs 满足需求，避免复杂化 |
| 凭据入库/加密改造 | credentials.json 保持现状（本期范围外，后续可单独评估加密） |
| 健康评分系统（kiro-sub 式 0-100 分） | 现有 priority + 冷却 + 余额调度已够用；失败率展示先行，评分调度后续评估 |
| Prompt Cache 计量模拟（ZyphrZero 1299 行） | 复杂度高，收益（响应中的 cache 字段）非刚需 |

## 6. 验证计划

**单元测试**（cargo test）：
- `map_model`：新模型全变体（base/thinking/agentic/[1m]/日期后缀）+ fable 分支 + 未知模型返回 None
- effort：5 值/4 值/不支持模型的降级矩阵；无效字符串丢弃；未知新模型放行 xhigh
- 响应模型 ID：always-1M 补 `[1m]`、已带后缀不重复追加
- StatsEntry：老 JSON 缺字段反序列化（serde default）；跨日 daily_count 重置
- CooldownEntry：序列化往返；过期条目启动时丢弃

**集成测试**：
- SQLite 日志：正常流结束 / 客户端断开 / 上游中断三种路径都产生完整记录
- 多次重试的 attempt 明细完整性（触发 429 → 故障转移 → 成功）

**手动验证**（make dev + Claude Code 实测）：
- `claude-sonnet-5` / `claude-opus-4-8` 实际对话，确认响应 model 字段带 `[1m]` 且 Claude Code 识别 1M 上下文
- `claude-fable-5` 直通请求，观察上游行为（可能 400，确认错误信息清晰）
- Admin UI 四 Tab 全流程走查；冷却倒计时实时性

## 7. 风险与兼容性

| 风险 | 缓解 |
|---|---|
| Fable 5 真实 SKU 未知，直通可能 400 | 明确错误提示；SKU 确认后一行改动即修正 |
| effort 表可能随 kiro-cli 版本变化 | 以 kirocc（kiro-cli 2.10.0 schema）为基线；未知模型乐观放行，降低维护频率 |
| `[1m]` 后缀影响现有客户端 | 仅对 always-1M 模型追加；现有 4.5/4.6 短上下文模型行为不变 |
| rusqlite 引入编译依赖 | 用 `bundled` feature 静态编译，无系统依赖；日志可通过配置关闭 |
| kiro_stats.json 升级 | serde default 全覆盖 + 保留原子写；老文件无损升级 |
| 日志写入拖慢热路径 | mpsc 异步化 + 满时丢弃；日志故障不影响代理 |
| dashboard.tsx 重构回归 | 先拆文件不改逻辑（纯移动），再增新功能，分两个提交 |

## 8. 实施顺序与工作量估算

```
阶段 1（P0 模型层）           约 1 天
  ├── converter.rs 新模型映射 + 单测
  ├── handlers.rs /v1/models 条目
  ├── effort.rs 新模块 + 单测
  └── stream.rs/handlers.rs 响应模型 ID 修正

阶段 2（P1 数据层）           约 2 天
  ├── trace.rs TraceSink trait + mpsc 写入管道
  ├── trace_db.rs SQLite 存储 + 清理任务
  ├── provider.rs 埋点（on_attempt / finish）
  ├── stream.rs 三种结束路径的 flush
  ├── StatsEntry 增强 + 跨日重置
  ├── cooldown.rs 长冷却持久化
  └── Admin API 四端点

阶段 3（P2 展示层）           ✅ 已完成
  ├── dashboard.tsx 拆分 4 Tab（纯重构提交）
  ├── 请求日志 Tab + Collapsible 明细
  ├── 统计概览 Tab + 趋势图
  └── 凭据卡片冷却倒计时/统计增强

阶段 4（P3 健壮性）           ✅ 已存在
  └── upstream Converter 增强移植（对比去重后）

总计约 6 个工作日；每阶段独立可交付、可单独验证。
```

依赖关系：阶段 3 依赖阶段 2 的 API；阶段 1 / 4 完全独立，可并行或调序。
