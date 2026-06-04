<!-- generated-by: gsd-doc-writer -->
# jadepaw 整体架构

**版本:** 0.1.0
**最后更新:** 2026-06-04
**状态:** Phase 4 完成后，反映当前实现与已规划的中远期扩展

---

## 1. 架构概览

jadepaw 是一个通用 AI Agent 运行时平台，核心理念是将"Skill"视为自然语言程序，底层基于 WebAssembly 实现硬件级多租户隔离。

### 1.1 设计原则

| 原则 | 说明 |
|------|------|
| **分层架构，单向依赖** | 下层不感知上层，依赖方向不可逆 |
| **Wasm 硬件级隔离** | Store-per-session，PoolingAllocator 保证内存不交叉 |
| **默认拒绝安全模型** | 能力白名单，未显式授权的操作在副作用发生前被拦截 |
| **Additive-only 扩展** | HostFunctions trait 只能加方法，不能删；ResourceLimiter 委托链可无限扩展 |
| **Web-first，零构建** | HTMX + SSE，无需 npm 构建步骤 |

### 1.2 核心指标

| 指标 | 目标 | 当前 |
|------|------|------|
| 冷启动延迟 | ≤5ms P99 | ~0-2ms avg (Phase 2 benchmark) |
| 单机并发实例 | ≥10,000 | 架构支持，待压力验证 |
| 单实例内存 | ≤64MB | 64MB 硬限制 (InstanceHardLimiter) |
| 租户数据泄漏 | 零 | PoolingAllocator 线性内存硬边界 |

---

## 2. Crate 组织与依赖

### 2.1 完整依赖图

```
jadepaw-core          ← 零内部依赖，全局基础类型
    ↑
jadepaw-wasm          ← 只依赖 core，Wasm 运行时集成
    ↑
jadepaw-bus           ← 依赖 core + wasm（stub）
    ↑          ↑
    │          ├──── jadepaw-agent  ← 依赖 core + wasm
    │          │
    │          └──── jadepaw-gateway ← 依赖 core + wasm + bus（stub）
    │                              ↑
    │                              │
    └──── jadepaw-skill  ← 依赖 core + wasm + agent（stub）
                                      ↑
                                      │
                                jadepaw-server  ← 依赖全部（binary crate，stub）
```

### 2.2 Crate 职责与实现状态

| Crate | 类型 | 职责 | 状态 |
|-------|------|------|------|
| `jadepaw-core` | library | 共享类型、错误、HostFunctions trait、Tool trait、能力白名单、ReActStep 等 Agent 类型 | **Phase 4 完成** |
| `jadepaw-wasm` | library | wasmtime Engine、Store、ResourceLimiter链、host函数、实例池、Tool 实现 (HttpRequest, FileRead, FileWrite) | **Phase 4 完成** |
| `jadepaw-agent` | library | ReAct Agent Loop、LLM 客户端 (async-openai with SSE streaming)、SSE 流式输出、终止守卫、ToolRegistry | **Phase 4 完成** |
| `jadepaw-bus` | library | Agent/Skill 间消息总线、工作流 DAG | Phase 5+ stub |
| `jadepaw-skill` | library | SKILL.md 解析、热加载、版本管理 | Phase 6+ stub |
| `jadepaw-gateway` | library | HTTP/SSE/WS 端点、认证、租户路由 | Phase 7+ stub |
| `jadepaw-server` | binary | 组装所有 crate，启动 axum 服务 | Phase 7+ stub |

---

## 3. 逐层架构

### 3.1 Layer 1: 基础类型（jadepaw-core）

所有 crate 的共同基础。零内部依赖，零 wasmtime 依赖。

```
jadepaw-core/src/
├── types.rs              强类型 ID (UUID v7 newtype)
│   ├── SessionId          会话标识
│   ├── TenantId           租户标识
│   └── ToolId             工具标识
│
├── capabilities.rs        能力白名单（默认拒绝语义）
│   ├── InstanceCapabilities
│   │   ├── can_read_files:   Vec<PathPattern>
│   │   ├── can_write_files:  Vec<PathPattern>
│   │   ├── can_exec_tools:   Vec<ToolId>
│   │   ├── can_network_to:   Vec<DomainPattern>
│   │   ├── max_memory_mb:    u32          (默认 64)
│   │   └── max_compute_units: u64         (默认 0)
│   ├── PathPattern          "data/*", "*.md"
│   └── DomainPattern        "*.example.com"
│
├── host_functions.rs       Guest-Host 通信契约
│   HostFunctions trait (#[async_trait], additive-only)
│   ├── log_message(level, msg)           → Result<()>
│   ├── file_read(path)                   → Result<Vec<u8>>
│   ├── file_write(path, data)            → Result<()>
│   ├── http_request(method, url, headers, body) → Result<(u16, headers, body)>   (Phase 4)
│   └── (spawn_wasm ...)                  → 远期 JIT
│
├── tool.rs                 Agent 级 Tool 抽象（Phase 4）
│   ├── Tool trait           name(), description(), input_schema(), call()
│   ├── ToolResult           结构化成功/错误（LLM 可消费）
│   ├── ToolDefinition       MCP 兼容格式 (name, description, inputSchema)
│   └── extract_host_from_url()  共享的 host 提取工具函数
│
├── agent_types.rs          Agent 运行时数据结构（Phase 3）
│   ├── AgentRequest         session_id, user_message, context
│   ├── AgentResponse        session_id, final_answer, trace
│   ├── ReActStep enum       Thought, Action, Observation, Error, Finished
│   └── AgentTerminationReason   MaxIterations, WallClockTimeout, WasmTrap, InfrastructureError
│
├── guest_exports.rs        Guest 导出接口定义
│   └── NextAction           ToolChoice, ToolDef
│
└── error.rs                统一错误类型
    JadepawError { CapabilityDenied, TrapError, PathValidationError, AgentTerminated, ... }
```

**关键设计约束**：HostFunctions trait 和 Tool trait 均在 jadepaw-core 中定义（不依赖 jadepaw-wasm），使得 jadepaw-agent、jadepaw-skill 等上层 crate 可以引用 trait 而无需依赖 wasmtime。

### 3.2 Layer 2: Wasm 隔离运行时（jadepaw-wasm）

架构的底盘。已完整实现，Phase 4 新增了 Tool 实现和共享 HTTP client。

```
jadepaw-wasm/src/
│
├── engine.rs             EngineFactory::build()
│   ├── Fuel metering (consume_fuel = true)
│   ├── Epoch interruption (epoch_interruption = true)
│   ├── PoolingAllocator (64MB slots, 100 warm slots)
│   └── Cranelift OptLevel::Speed
│
├── session.rs            SessionState — Store<T> 的 T
│   ├── session_id, tenant_id
│   ├── capabilities: InstanceCapabilities
│   ├── limits: SessionLimits → InstanceHardLimiter
│   ├── sandbox_root: PathBuf
│   ├── http_client: reqwest::Client       ← Phase 4 新增，共享复用
│   └── created_at: DateTime<Utc>
│
├── limits/
│   ├── instance_hard.rs  InstanceHardLimiter (64MB → Err, Store 毒化)
│   ├── tenant_quota.rs   TenantQuotaLimiter (预算 → Ok(false), 可恢复)
│   └── mod.rs            Delegating Chain 模式，可无限扩展
│       未来: ToolRateLimiter / ProcessLimiter / NetworkLimiter
│
├── epoch.rs              start_epoch_ticker(engine) → EpochTickerGuard
│   后台线程 ~1ms 递增 epoch，EngineWeak 自动退出
│
├── path.rs               路径安全
│   ├── normalize_path()         折叠 ../ 和 //
│   ├── validate_sandbox_path()  canonicalize + 前缀校验
│   └── TOCTOU 窗口已文档化
│
├── capability/mod.rs     SessionState 上的 can_* 方法
│   ├── can_read_file(path)      通配符模式匹配
│   ├── can_write_file(path)     通配符模式匹配
│   ├── can_call_tool(id)        精确匹配
│   └── can_access_domain(domain) 通配符前缀匹配
│
├── linker.rs             create_linker + register_host_functions
│   在 "jadepaw" 命名空间下注册所有 host 函数
│   func_wrap_async 用于所有 I/O host 函数
│
├── host/
│   ├── logging.rs        log_message — 始终允许，路由到 tracing
│   ├── filesystem.rs     file_read / file_write — 能力检查 → 路径校验 → I/O
│   └── network.rs        http_request_host_fn — Phase 4 完整实现:
│        ├── Scheme 校验 (仅 http/https)
│        ├── Domain 能力白名单检查
│        ├── SSRF IP 层检查 (resolve_and_check_ssrf_addr — 共享函数)
│        ├── DNS 超时 5s，请求超时 30s
│        ├── 禁止头部过滤 (host, content-length, transfer-encoding 等 6 个)
│        ├── Userinfo 剥离 (WR-04: 防止凭证泄漏到代理日志)
│        ├── 共享 SessionState::http_client (CR-01: 不复用连接泄漏)
│        └── 返回 status_code (i32)，丢弃 response body
│
├── tool_impls/           Phase 4 新增: Tool trait 具体实现
│   ├── http_tool.rs      HttpRequestTool — 7 层 SSRF 防御:
│   │   ├── 1. Scheme 校验 (仅 http/https)
│   │   ├── 2. Domain 白名单 (ToolRegistry 层执行)
│   │   ├── 3. IP 层 SSRF: resolve_and_check_ssrf + is_blocked_ip
│   │   ├── 4. Redirect 限制 (Policy::limited(1))
│   │   ├── 5. Response body cap (1MB chunked 增量读取)
│   │   ├── 6. Timeout 30s
│   │   ├── 7. 禁止头部过滤 + CR/LF 注入检测
│   │   └── 已知风险: DNS rebinding TOCTOU (已文档化，接受 MVP 风险)
│   ├── file_tool.rs      FileReadTool / FileWriteTool
│   │   复用 Phase 2 sandbox 路径校验 (validate_sandbox_path)
│   │   返回结构化 ToolResult (非 raw i32)
│   └── mod.rs            模块组织
│
└── pool.rs               InstancePool — 核心编排器
    ├── PoolConfig { guest_bytes, sandbox_root, max_concurrent }
    ├── new() → Engine + Module + Linker + Arc<InstancePre>
    └── acquire(session_id, state) → Semaphore → Store::new → fuel
        → epoch → limiter → instantiate_async → SessionHandle
```

#### 3.2.1 ResourceLimiter 委托链

```
┌─────────────────────────────────────────────┐
│  guest 调用 memory.grow(n)                    │
│           ↓                                  │
│  ┌──────────────────────────────────────┐    │
│  │ TenantQuotaLimiter                    │    │
│  │ 共享 Arc<AtomicUsize> 租户预算计数器   │    │
│  │ 超预算 → Ok(false), guest 收到 -1      │    │
│  │ 未超   → 委托内层                      │    │
│  └──────────────┬───────────────────────┘    │
│                 ↓                            │
│  ┌──────────────────────────────────────┐    │
│  │ InstanceHardLimiter                   │    │
│  │ 64MB 硬上限                           │    │
│  │ 超上限 → Err(), Store 毒化 (不可恢复)  │    │
│  │ 未超   → Ok(true), 内存分配成功        │    │
│  └──────────────────────────────────────┘    │
│                                               │
│  未来可前置插桩 (不改变内层):                   │
│  ┌──────────────────────────────────────┐    │
│  │ ToolRateLimiter      (Phase 5+)      │    │
│  │ ProcessLimiter       (JIT harness)   │    │
│  │ NetworkLimiter       (Phase 5+)      │    │
│  │ ...                                  │    │
│  └──────────────┬───────────────────────┘    │
│                 ↓                            │
│            (继续委托到下层)                     │
└─────────────────────────────────────────────┘
```

#### 3.2.2 Host 函数安全执行流程

```
guest 调用 jadepaw.file_read("../../../etc/passwd", buf)
        │
        ▼
┌───────────────────────────────────────────────┐
│ 1. caller.data()         获取 SessionState    │
│ 2. memory.data_size()    边界检查 guest 指针   │
│ 3. can_read_file(path)   能力白名单检查         │  ← 任一失败返回 -1
│ 4. validate_sandbox_path 路径穿越防御          │     无副作用
│ 5. tokio::fs::read()     实际 I/O              │  ← 全部检查通过后
└───────────────────────────────────────────────┘
```

### 3.3 Layer 3: Agent 运行时（jadepaw-agent）— Phase 3/4 完成

```
jadepaw-agent/src/
├── loop.rs        ReAct 循环: Think → Act → Observe → (repeat)
│                  使用 real async-openai Client<Box<dyn Config>> 流式调用
│                  每轮重置 Fuel (1M)，解析 LLM 响应中的 ACTION/FINAL ANSWER 指令
│                  通过 ToolRegistry 分发 tool 调用，结果追加到 LLM 消息历史
│
├── llm.rs         LLM 客户端 (async-openai, Box<dyn Config>)
│                  build_initial_messages(), stream_llm_response() (SSE 流)
│                  parse_next_action() → LlmDirective (Finish, Act, ContinueThinking)
│                  build_system_prompt_with_tools() — 将 Tool 描述注入 system prompt
│                  REACT_SYSTEM_PROMPT — 硬编码 ReAct system prompt
│
├── stream.rs      SSE 流式输出
│                  mpsc::channel(256) → ReceiverStream → axum Sse
│                  ReActStep 映射到命名 SSE 事件 (thought, action, observation, error, done)
│                  自动检测客户端断开 (is_closed)，优雅停止流式输出
│
├── guard.rs       终止守卫: 最大迭代次数 (20) + 墙钟超时 (5min)
│                  tokio::select! 竞争 agent_loop 与 sleep
│                  LoopErrorKind 结构化错误分类 → AgentTerminationReason
│
├── tool_registry.rs  ToolRegistry — 集中式 Tool 注册与分发 (Phase 4)
│                  DashMap<ToolId, Arc<dyn Tool>> 无锁并发
│                  DashMap<String, ToolId> 名称索引 (O(1) 查找)
│                  call_tool(): 查找 → can_call_tool() 能力检查 → dispatch
│                  http_request domain 白名单在 Registry 层执行 (CR-01)
│                  返回 ToolId 消除 TOCTOU (WR-02)
│
└── lib.rs         入口: run_agent() — 组合 pool, llm_client, tool_registry
                   AgentRequest → (AgentResponse, SSE Stream)
```

#### 3.3.1 ReAct 循环数据流

```
用户消息
    │
    ▼
build_initial_messages(system_prompt, user_message, context)
    │ [system, user]
    ▼
┌─────────────────────────────────────────────────────┐
│  for turn in 0..max_iterations(20):                 │
│    1. Fuel reset (1M per turn)                      │
│    2. stream_llm_response(messages, close_signal)   │
│       → 累积 full_response + 检测 channel close      │
│    3. emit ReActStep::Thought                       │
│    4. parse_next_action(full_response)              │
│       ├── LlmDirective::Finish → Finished → 退出    │
│       ├── LlmDirective::Act(tool, args)             │
│       │   → ToolRegistry::call_tool() → Observation │
│       │   → append tool result + assistant msg      │
│       └── LlmDirective::ContinueThinking → 继续      │
└─────────────────────────────────────────────────────┘
    │
    ▼
AgentResponse { session_id, final_answer, trace }
```

### 3.4 Layer 4: 消息总线（jadepaw-bus）— Phase 5+ stub

```
jadepaw-bus/src/
├── lib.rs         Stub: 模块结构文档 (模块声明 + 职责说明)
│                  计划: channel.rs (Agent ↔ Agent 通信), dag.rs (工作流 DAG),
│                  pubsub.rs (跨 Session 事件: 单机 tokio::mpsc / 集群 Redis)
```

### 3.5 Layer 5: Skill 系统（jadepaw-skill）— Phase 6+ stub

```
jadepaw-skill/src/
├── lib.rs         Stub: 模块结构文档 (模块声明 + 职责说明)
│                  计划: parser.rs (SKILL.md YAML frontmatter), registry.rs (DashMap),
│                  loader.rs (热加载), version.rs (Git-based)
```

### 3.6 Layer 6: 接入层（jadepaw-gateway）— Phase 7+ stub

```
jadepaw-gateway/src/
├── lib.rs         Stub: 模块结构文档 (模块声明 + 职责说明)
│                  计划: http.rs (REST + SSE), ws.rs (WebSocket), auth.rs (API Key / JWT),
│                  tenant.rs (租户路由中间件), rate.rs (Rate Limiting)
```

### 3.7 Layer 7: 启动入口（jadepaw-server）— Phase 7+ stub

```
jadepaw-server/src/
├── main.rs        Stub: println!("jadepaw server starting...");
│                  计划: 组装 axum Router + tracing subscriber 初始化
└── lib.rs
```

---

## 4. 安全架构

### 4.1 多层防御模型

```
┌─────────────────────────────────────────────────────────┐
│  Layer 1: 能力白名单 (Capability Gating)                 │
│  can_read_file / can_write_file / can_call_tool          │
│        / can_access_domain                               │
│  ↓ 通过 → 继续; ↓ 拒绝 → -1 (无副作用)                    │
├─────────────────────────────────────────────────────────┤
│  Layer 2: 路径沙箱 (Path Sandbox)                        │
│  normalize → canonicalize → sandbox_root 前缀校验        │
│  ↓ 通过 → 继续; ↓ 拒绝 → -1 (无副作用)                    │
├─────────────────────────────────────────────────────────┤
│  Layer 3: 网络控制 (Network ACL) — Phase 4 实现          │
│  can_access_domain + DomainPattern 匹配                  │
│  + IP 层 SSRF (is_blocked_ip: private/loopback/link-     │
│    local/multicast/broadcast/unspecified/CGNAT)          │
│  + IPv4-mapped IPv6 检测 (防止 ::ffff:127.0.0.1 绕过)    │
│  + Scheme 校验 (仅 http/https)                            │
│  + 禁止头部过滤 + CR/LF 注入检测                          │
│  + Userinfo 剥离                                         │
│  + Redirect 限制 (最多 1 次)                              │
│  + Body cap 1MB + Timeout 30s + DNS timeout 5s           │
│  ↓ 通过 → 继续; ↓ 拒绝 → -1 (无副作用)                    │
├─────────────────────────────────────────────────────────┤
│  Layer 4: 资源硬限制 (Resource Limits)                    │
│  InstanceHardLimiter (64MB) + Fuel (1M) + Epoch (~1ms)  │
│  ↓ 超限 → Trap (Store 毒化, 不可恢复, 实例销毁)           │
├─────────────────────────────────────────────────────────┤
│  Layer 5: Wasm 线性内存硬件隔离                            │
│  PoolingAllocator 独立槽位, Store 销毁后内存归零           │
│  ↓ 保证租户间数据不可能泄漏                                │
└─────────────────────────────────────────────────────────┘
```

### 4.2 各层失败模式

| 层 | 触发条件 | 行为 | 副作用 | 可恢复性 |
|----|---------|------|--------|---------|
| 能力白名单 | can_* 返回 false | host fn 返回 -1 | 无 | 可恢复 |
| 路径沙箱 | 路径穿越检测 | host fn 返回 -1 | 无 | 可恢复 |
| 网络 ACL | 域名不在白名单 / SSRF IP / scheme | host fn 返回 -1 | 无 | 可恢复 |
| 禁止头部 | forbidden header / CR/LF 注入 | 头部被丢弃，请求继续 | 无 | 可恢复 |
| 硬限制 | memory > 64MB | Err(), Trap | Store 毒化 | 不可恢复 |
| 租户配额 | 租户总内存超预算 | Ok(false), guest 收到 -1 | 无 | 可恢复 |
| Fuel 耗尽 | 超过 1M 指令 | Trap | Store 毒化 | 不可恢复 |
| Epoch 中断 | 超过 deadline | Trap | Store 毒化 | 不可恢复 |

---

## 5. 扩展轴

架构围绕三条正交的扩展轴设计：

### 5.1 能力轴（HostFunctions trait）

```
HostFunctions trait (additive-only)

Phase 2  Phase 4   远期
   │        │        │
log_message│        │
file_read  │        │
file_write │        │
       http_request  │
                 spawn_wasm (JIT harness)
                 git_*      (git harness)
                 code_exec  (通用代码执行)
```

### 5.2 限制轴（ResourceLimiter 委托链）

```
ResourceLimiter chain (委托模式, 可前置插桩)

InstanceHardLimiter ── 不变的安全边界 ──┐
TenantQuotaLimiter ── 包装 hard ────────┤ 已实现
ToolRateLimiter ──── 前置插桩 ──────────┤ Phase 5+
ProcessLimiter ───── 前置插桩 ──────────┤ 远期 (JIT)
NetworkLimiter ───── 前置插桩 ──────────┤ Phase 5+
...                                      │ 任意扩展
```

### 5.3 隔离轴（InstancePool + SessionState）

```
InstancePool
  ├── Engine (单例, 编译缓存)
  ├── Module (预编译)
  ├── Linker (host 函数注册)
  ├── Arc<InstancePre> (共享模板)
  │
  └── acquire() per session:
        ├── Semaphore (并发上限)
        ├── Store::new() (独立 Store)
        ├── SessionState (独立能力/限制/sandbox/http_client)
        ├── set_fuel(1M) (独立 Fuel)
        ├── epoch_deadline (独立 Epoch)
        ├── store.limiter() (独立 ResourceLimiter)
        └── instantiate_async (独立 Instance)

  远期 JIT spawn:
    同样流程, Module::new(dynamic_bytes) 替代 InstancePre
    子实例 SessionState 从父实例 Clone 后收缩能力
```

---

## 6. 数据流

### 6.1 用户请求 → Agent 执行

```
浏览器                     Gateway               Agent Loop            Wasm Instance
  │                           │                      │                      │
  │ POST /chat/{session_id}   │                      │                      │
  │ {"message":"读 README"}   │                      │                      │
  │ ─────────────────────────►│                      │                      │
  │                           │ 认证 + 租户路由       │                      │
  │                           │ 查找 session          │                      │
  │                           │ ────────────────────►│                      │
  │                           │                      │ ReAct Loop 启动       │
  │                           │                      │ Think: 调用 LLM       │
  │                           │                      │ LLM: "需要读 README"   │
  │                           │                      │                      │
  │                           │                      │ Act: file_read        │
  │                           │                      │ ("README.md")         │
  │                           │                      │ ────────────────────►│
  │                           │                      │                      │
  │                           │                      │              can_read_file()
  │                           │                      │              validate_sandbox()
  │                           │                      │              tokio::fs::read()
  │                           │                      │              ← 文件内容
  │                           │                      │ ◄────────────────────│
  │                           │                      │                      │
  │                           │                      │ Observe: 分析内容     │
  │                           │                      │ Think: 生成回答       │
  │                           │                      │                      │
  │ SSE: data: {"token":"t"}  │                      │                      │
  │ ◄─────────────────────────│◄─────────────────────│                      │
  │                           │                      │                      │
  │ SSE: data: [DONE]         │                      │                      │
  │ ◄─────────────────────────│                      │                      │
  │                           │                      │                      │
  │                    SessionHandle Drop                                    │
  │                    Store 销毁, 内存槽位归零回收                           │
```

### 6.2 Session 生命周期

```
PoolConfig 创建
    │
    ▼
InstancePool::new()
    │ Engine + Module + Linker + Arc<InstancePre>
    │
    ▼
acquire(session_id, session_state)
    │ Semaphore 获取槽位 → Store::new → 配置 → 实例化
    │
    ▼
SessionHandle (活跃 session)
    │ Agent Loop 运行 ...
    │ guest 调用 host 函数 → 能力检查 → 路径校验 → I/O
    │
    ▼
SessionHandle Drop
    │ DashMap 移除 session 记录
    │ Store + Instance + Permit 释放
    │ PoolingAllocator 归零内存槽位
```

---

## 7. 技术栈映射

| 架构层 | 关键依赖 | 版本 |
|--------|---------|------|
| Wasm 运行时 | wasmtime (PoolingAllocator, Cranelift, Async) | 45.0 |
| 异步运行时 | tokio (multi-thread, work-stealing) | 1.52 |
| Web 框架 | axum (HTTP, WS, SSE) | 0.8 |
| LLM 客户端 | async-openai (Box<dyn Config>) | 0.40 |
| HTTP 客户端 | reqwest (SSRF 防御, ConnectionPool) | (async-openai transitive) |
| 数据库 | SQLx (SQLite 单机, PostgreSQL 集群) | 0.9 |
| 缓存 | redis-rs (session 状态, pub/sub) | 1.2 |
| 前端 | HTMX + SSE (零构建) | 2.0 |
| 序列化 | serde + serde_json | 1.0 |
| ID 生成 | uuid (v7, 时间有序) | 1.0 |
| 时间处理 | chrono | 0.4 |
| 并发数据结构 | dashmap | 6 |
| 可观测性 | tracing + tracing-subscriber | 0.1 / 0.3 |
| Tokio 流适配 | tokio-stream (ReceiverStream) | 0.1 |

---

## 8. Phase 与架构的对应关系

```
Phase 1  ████░░░░░░░░░░░░░░░░░░  项目骨架 + CI              ← 完成
Phase 2  ████████████░░░░░░░░░░  Wasm 隔离核心 (架构底盘)     ← 完成
Phase 3  ████████████████░░░░░░  Agent Loop + LLM + SSE     ← 完成
Phase 4  ██████████████████░░░░  Tool System + SSRF 防御     ← 完成
Phase 5  ████████████████████░░  Session 持久化 + Bus 实现
Phase 6  ██████████████████████  Skill 系统 + 热加载
Phase 7  ██████████████████████  Web Chat UI + Gateway + Server
Phase 8  ██████████████████████  Skill 管理 UI
Phase 9  ██████████████████████  可观测性
```

各 Phase 的实现仅在上层对应 crate 中增加代码，不会改变 core 和 wasm 的接口（仅在 additive-only 约束下扩展 trait 和注册新函数）。

### 8.1 Phase 3 成果（Agent Runtime）

- **ReAct 循环**: Think → Act → Observe 完整实现，通过 ToolRegistry 分发 tool 调用
- **LLM 集成**: async-openai 流式调用，Box<dyn Config> 动态 provider 切换
- **SSE 流式输出**: mpsc channel → ReceiverStream → axum Sse，支持优雅断连检测
- **终止守卫**: 最大 20 轮迭代 + 5 分钟墙钟超时 + LoopErrorKind 结构化错误分类
- **ReActStep 枚举**: Thought, Action, Observation, Error, Finished — 完整执行 trace
- **测试覆盖**: agent_types 单元测试, host_functions 测试, SSE streaming 测试, agent_loop 测试

### 8.2 Phase 4 成果（Tool System）

- **Tool trait**: 定义在 jadepaw-core，name/description/inputSchema/call()，MCP 兼容
- **ToolRegistry**: DashMap 无锁并发，名称索引 O(1) 查找，capability 门控
- **HttpRequestTool**: 7 层 SSRF 防御 (scheme/domain/IP/redirect/body-cap/timeout/headers)
- **FileReadTool / FileWriteTool**: 复用 Phase 2 sandbox 路径校验，结构化 ToolResult
- **Host 函数 network.rs**: 从 stub 升级为完整实现 (上述 7 层防御均生效)
- **SessionState.http_client**: 共享 reqwest::Client (CR-01: 避免 per-call 资源泄漏)
- **is_blocked_ip**: IPv4/IPv6 全覆盖，IPv4-mapped IPv6 检测，CGNAT 屏蔽
- **resolve_and_check_ssrf_addr**: Host 函数与 Tool 路径共享的 SSRF DNS 检查函数

---

## 9. 与早期设计文档的关系

- **`docs/jadepaw_discussion.md`** — 技术选型和架构决策的讨论记录，解释 "为什么这样设计"
- **`docs/arch.mermaid`** — 早期概念级 mermaid 图
- **本文档 (`docs/architecture.md`)** — 实现级架构总览，反映实际代码结构和已敲定的扩展计划

三份文档互补：discussion.md 回答 "why"，architecture.md 回答 "what"，代码本身回答 "how"。