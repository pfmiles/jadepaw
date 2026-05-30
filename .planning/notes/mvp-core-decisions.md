---
title: "MVP核心设计决策"
date: 2026-05-28
context: "基于jadepaw_discussion.md的脑暴讨论，明确了MVP形态、Agent Loop设计、Skill自举机制等关键决策"
---

## 产品定位

jadepaw是一个**可直接被最终用户使用的通用Agent引擎**，核心理念是"Skill即自然语言程序"——用户无需编程能力，通过自然语言即可定制Agent行为。

与Claude Code/OpenClaw的根本差异：**个人创作 → 交互式精炼 → 一键发布为多租户企业服务**，打通了"个人Agent"到"企业级Agent平台"的完整闭环。

## MVP形态决策

**选择：HTTP API + 内置Web Chat**

- 基于内置Web服务器（Rust/tokio/axum），统一本地使用和远程企业部署的UI代码
- 本地场景：`jadepaw serve` → 浏览器打开 `localhost:PORT` 使用Web Chat
- 企业场景：同一套Web UI部署到服务器，多租户通过Session隔离

## Agent Loop设计决策

**选择：混合模式（粗粒度规划 + ReAct执行）**

- 第一阶段：LLM生成高层计划（3-7个阶段性步骤）
- 第二阶段：每个步骤内部用ReAct循环执行（think → tool → observe → next）
- 计划偏离时触发局部重规划，不推翻整个任务
- 用户可实时看到进度："步骤2/5：汇总飞书数据"

优势：结合了目标导向性和执行灵活性，行为更像"有计划的机器人"。

## Skill自举机制

**核心理念：交互式Skill创建 = Agent的第一项内置Skill**

流程：对话引导 → 提取意图 → 生成结构化Skill草稿 → Wasm沙箱安全预览 → 迭代精炼 → 发布/部署

关键差异点：Wasm沙箱允许用户在隔离环境中**安全预览**未完成的Skill，这是GPT Builder和Claude Code都不具备的能力。

## Skill格式

采用声明式Markdown/YAML结构（借鉴Claude Code + OpenClaw模式）：
- 结构化骨架：name, description, trigger, tools, constraints
- 逻辑部分：自然语言指令填充
- 可版本控制、可分享、机器可读

## MVP最小可用定义

"可用"标准：对话输入 → Agent Loop规划并执行 → 产生可观测结果

不必贪多求全——先跑通最小闭环，后续迭代持续丰富功能。

## LLM 抽象层设计 (Phase 3)

**选择：`LlmClient` trait 统一多厂商后端**

当前 `async-openai` 的 `Box<dyn Config>` 可以覆盖所有 OpenAI 兼容 API（OpenAI、Azure、Ollama、DeepSeek、Groq 等），但 Anthropic 的 Messages API 与 OpenAI 协议不兼容，无法走同一条路径。

在 Phase 3 实现 Agent Runtime 时，在 `jadepaw-agent` 中定义 `LlmClient` trait：

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse>;
    async fn chat_stream(&self, req: ChatRequest) -> Result<Stream>;
}
```

两个实现：
- `OpenAiBackend` — 内部用 `async-openai`，覆盖所有 OpenAI 兼容服务（含本地 Ollama/vLLM）
- `AnthropicBackend` — 内部用 Anthropic SDK 或直接 HTTP，覆盖 Anthropic Messages API

Agent 的 Planner 和 ReAct executor 只依赖 `LlmClient` trait，不感知具体厂商。**不用编译期 feature flag 区分厂商**——运行时通过配置选择后端，与 D-05 一致。

## 存储层抽象设计 (Phase 5)

**选择：`Storage` trait 统一多数据库后端，不写死 PostgreSQL**

单节点模式默认使用 SQLite（零配置、单文件、开箱即用），面向个人用户。集群模式面向有技术能力的企业用户，通过 trait 抽象允许适配任意关系型数据库，而非锁死在 PostgreSQL 上。

在 Phase 5 实现 Session Memory 时，在 `jadepaw-core` 中定义 `Storage` trait：

```rust
#[async_trait]
pub trait Storage: Send + Sync {
    // Session
    async fn save_session(&self, session: &Session) -> Result<()>;
    async fn load_session(&self, id: &SessionId) -> Result<Option<Session>>;
    async fn list_sessions(&self, tenant_id: &TenantId) -> Result<Vec<SessionSummary>>;

    // Tenant config
    async fn save_tenant_config(&self, config: &TenantConfig) -> Result<()>;
    async fn load_tenant_config(&self, id: &TenantId) -> Result<Option<TenantConfig>>;

    // Audit log
    async fn append_audit_log(&self, entry: &AuditEntry) -> Result<()>;
    async fn query_audit_logs(&self, filter: &AuditFilter) -> Result<Vec<AuditEntry>>;
}
```

内置两个实现：
- `SqliteStorage` — 基于 sqlx + SQLite，单节点默认，零配置开箱即用
- `PostgresStorage` — 基于 sqlx + PostgreSQL，集群模式参考实现

企业用户可以自行实现 `Storage` trait 对接 MySQL、TiDB、CockroachDB 等，无需修改 jadepaw 核心代码。**不与 sqlx 强绑定**——trait 方法是异步的，返回的是 jadepaw 内部类型，实现者可以选用 Diesel、SeaORM 或裸 driver。

## Redis 不做抽象 (Phase 5)

**选择：直接使用 `redis-rs`，不引入额外 trait 层**

评估结论：Redis 不需要类似 `Storage` 的抽象层。原因：

1. **RESP 协议是事实标准**——Valkey、KeyDB、Dragonfly、Garnet、AWS ElastiCache、GCP Memorystore 全部兼容，换后端只需改连接字符串
2. **jadepaw 只使用标准 RESP 命令**（GET/SET/DEL + PUBLISH/SUBSCRIBE + SETNX/EXPIRE），不依赖 Redis 专有模块
3. **`redis-rs` 的 `ConnectionManager` 已提供足够的抽象**——连接池、重连、pipeline 开箱即用
4. **真正的异构替换（Kafka/etcd）是架构模式变更，不是 KV trait 能解决的**

不同于关系型数据库（PostgreSQL ≠ MySQL ≠ SQLite，协议和方言完全不同）和 LLM API（OpenAI ≠ Anthropic，请求/响应格式不同），Redis 生态的协议统一性使得抽象层只有成本没有收益。