# jadepaw 整体架构

**版本:** 0.1.0
**最后更新:** 2026-05-31
**状态:** Phase 2 完成后，反映当前实现与已规划的中远期扩展

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
jadepaw-bus           ← 依赖 core + wasm
    ↑          ↑
    │          ├──── jadepaw-agent  ← 依赖 core + wasm + bus
    │          │
    │          └──── jadepaw-gateway ← 依赖 core + wasm + bus
    │                              ↑
    │                              │
    └──── jadepaw-skill  ← 依赖 core + wasm + agent
                              ↑
                              │
                        jadepaw-server  ← 依赖全部 (binary crate)
```

### 2.2 Crate 职责与实现状态

| Crate | 类型 | 职责 | 状态 |
|-------|------|------|------|
| `jadepaw-core` | library | 共享类型、错误、HostFunctions trait、能力白名单 | **Phase 2 完成** |
| `jadepaw-wasm` | library | wasmtime Engine、Store、ResourceLimiter链、host函数、实例池 | **Phase 2 完成** |
| `jadepaw-agent` | library | ReAct Agent Loop、LLM 客户端、流式输出 | Phase 3 实现 |
| `jadepaw-bus` | library | Agent/Skill 间消息总线、工作流 DAG | Phase 3+ 实现 |
| `jadepaw-skill` | library | SKILL.md 解析、热加载、版本管理 | Phase 6 实现 |
| `jadepaw-gateway` | library | HTTP/SSE/WS 端点、认证、租户路由 | Phase 7 实现 |
| `jadepaw-server` | binary | 组装所有 crate，启动 axum 服务 | Phase 7 实现 |

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
│   ├── tool_call(name, params)           → Result<Value>   (Phase 4)
│   └── spawn_wasm(bytes, caps)           → Result<Handle>  (远期 JIT)
│
└── error.rs               统一错误类型
    JadepawError { CapabilityDenied, TrapError, PathValidationError }
```

**关键设计约束**：HostFunctions trait 在 jadepaw-core 中定义（不依赖 jadepaw-wasm），使得 jadepaw-agent、jadepaw-skill 等上层 crate 可以引用 trait 而无需依赖 wasmtime。

### 3.2 Layer 2: Wasm 隔离运行时（jadepaw-wasm）

架构的底盘。已完整实现。

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
│   └── network.rs        http_request — Phase 2 stub, Phase 4 激活
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
│  │ ToolRateLimiter      (Phase 4)       │    │
│  │ ProcessLimiter       (JIT harness)   │    │
│  │ NetworkLimiter       (Phase 4)       │    │
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

### 3.3 Layer 3: Agent 运行时（jadepaw-agent）— Phase 3

```
jadepaw-agent/src/
├── loop.rs        ReAct 循环: Think → Act → Observe → (repeat)
├── llm.rs         LLM 客户端 (async-openai, Box<dyn Config>)
├── stream.rs      SSE 流式输出
└── guard.rs       终止守卫: 最大迭代次数 (20) + 墙钟超时 (5min)
```

### 3.4 Layer 4: 消息总线（jadepaw-bus）— Phase 3+

```
jadepaw-bus/src/
├── channel.rs     Agent ↔ Agent 通信
├── dag.rs         Skill 工作流 DAG 编排
└── pubsub.rs      跨 Session 事件: 单机 tokio::mpsc / 集群 Redis
```

### 3.5 Layer 5: Skill 系统（jadepaw-skill）— Phase 6

```
jadepaw-skill/src/
├── parser.rs      SKILL.md 解析 (YAML frontmatter + Markdown body)
├── registry.rs    Skill 注册表 (DashMap<SkillId, Skill>)
├── loader.rs      热加载 / 卸载
└── version.rs     版本管理 (Git-based)
```

### 3.6 Layer 6: 接入层（jadepaw-gateway）— Phase 7

```
jadepaw-gateway/src/
├── http.rs        REST API + SSE 端点
├── ws.rs          WebSocket 双向通信
├── auth.rs        认证 (API Key / JWT)
├── tenant.rs      租户路由中间件
└── rate.rs        Rate Limiting (tower layer)
```

### 3.7 Layer 7: 启动入口（jadepaw-server）— Phase 7

```
jadepaw-server/src/
├── main.rs        组装 axum Router + tracing subscriber 初始化
└── config.rs      全局配置 (监听地址、LLM provider、存储后端)
```

---

## 4. 安全架构

### 4.1 多层防御模型

```
┌─────────────────────────────────────────────────────────┐
│  Layer 1: 能力白名单 (Capability Gating)                 │
│  can_read_file / can_write_file / can_call_tool          │
│  ↓ 通过 → 继续; ↓ 拒绝 → -1 (无副作用)                    │
├─────────────────────────────────────────────────────────┤
│  Layer 2: 路径沙箱 (Path Sandbox)                        │
│  normalize → canonicalize → sandbox_root 前缀校验        │
│  ↓ 通过 → 继续; ↓ 拒绝 → -1 (无副作用)                    │
├─────────────────────────────────────────────────────────┤
│  Layer 3: 网络控制 (Network ACL) — Phase 4               │
│  can_access_domain + DomainPattern 匹配                  │
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
| 网络 ACL | 域名不在白名单 | host fn 返回 -1 | 无 | 可恢复 |
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
       tool_call ────┤
       tool_list     │
                 spawn_wasm (JIT harness)
                 git_*      (git harness)
                 code_exec  (通用代码执行)
```

### 5.2 限制轴（ResourceLimiter 委托链）

```
ResourceLimiter chain (委托模式, 可前置插桩)

InstanceHardLimiter ── 不变的安全边界 ──┐
TenantQuotaLimiter ── 包装 hard ────────┤ 已实现
ToolRateLimiter ──── 前置插桩 ──────────┤ Phase 4
ProcessLimiter ───── 前置插桩 ──────────┤ 远期 (JIT)
NetworkLimiter ───── 前置插桩 ──────────┤ Phase 4
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
        ├── SessionState (独立能力/限制/sandbox)
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
| 数据库 | SQLx (SQLite 单机, PostgreSQL 集群) | 0.9 |
| 缓存 | redis-rs (session 状态, pub/sub) | 1.2 |
| 前端 | HTMX + SSE (零构建) | 2.0 |
| 序列化 | serde + serde_json | 1.0 |
| ID 生成 | uuid (v7, 时间有序) | 1.0 |
| 时间处理 | chrono | 0.4 |
| 并发数据结构 | dashmap | 6 |
| 可观测性 | tracing + tracing-subscriber | 0.1 / 0.3 |

---

## 8. Phase 与架构的对应关系

```
Phase 1  ████░░░░░░░░░░░░░░░░░░  项目骨架 + CI              ← 完成
Phase 2  ████████████░░░░░░░░░░  Wasm 隔离核心 (架构底盘)     ← 完成
Phase 3  ████████████████░░░░░░  Agent Loop                  ← 下一阶段
Phase 4  ██████████████████░░░░  Tool/MCP 系统
Phase 5  ████████████████████░░  Session 持久化
Phase 6  ██████████████████████  Skill 系统 + 热加载
Phase 7  ██████████████████████  Web Chat UI + Gateway
Phase 8  ██████████████████████  Skill 管理 UI
Phase 9  ██████████████████████  可观测性
```

各 Phase 的实现仅在上层对应 crate 中增加代码，不会改变 core 和 wasm 的接口（仅在 additive-only 约束下扩展 trait 和注册新函数）。

---

## 9. 与早期设计文档的关系

- **`docs/jadepaw_discussion.md`** — 技术选型和架构决策的讨论记录，解释 "为什么这样设计"
- **`docs/arch.mermaid`** — 早期概念级 mermaid 图
- **本文档 (`docs/architecture.md`)** — 实现级架构总览，反映实际代码结构和已敲定的扩展计划

三份文档互补：discussion.md 回答 "why"，architecture.md 回答 "what"，代码本身回答 "how"。