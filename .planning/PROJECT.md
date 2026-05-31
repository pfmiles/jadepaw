# jadepaw

## What This Is

jadepaw 是一个可直接被最终用户使用的通用 AI Agent 引擎。核心理念是将"Skill"视为自然语言程序——用户无需传统编程能力，通过自然语言即可定制 Agent 行为，并可一键将个人创作的 Agent 发布为多租户企业级服务。底层基于 WebAssembly 实现强隔离、高密度的多租户架构。

## Core Value

让任何人都能用自然语言"编程"自己的 AI Agent，并将它部署为可供成百上千人同时使用的企业级服务。

## Requirements

### Validated

- ✓ **REQ-SECURITY-001**: Wasm 多租户实例隔离（独立 Memory/Table 对象，线性内存硬边界） — Phase 2
- ✓ **REQ-SECURITY-002**: 工具执行沙箱（路径标准化 + sandbox 前缀校验 + canonicalize） — Phase 2
- ✓ **REQ-SECURITY-003**: 能力白名单（默认拒绝，显式授权） — Phase 2

### Active

- [ ] **REQ-SECURITY-004**: 网络访问控制（白名单域名 + SSRF 防护） — Phase 2 stub (http_request always returns CapabilityDenied), full impl in Phase 4
- [ ] **REQ-AGENT-002**: 工具调用能力，MCP 兼容协议，MVP 至少支持文件读写和 HTTP 请求
- [ ] **REQ-AGENT-003**: 多 Agent 编排（router → specialist 模式），子 Agent 独立工具集和上下文
- [ ] **REQ-AGENT-004**: 流式输出（token 级），用户实时看到 Agent 思考和执行进展
- [ ] **REQ-MEMORY-001**: 短期记忆（单会话上下文管理、窗口压缩）
- [ ] **REQ-MEMORY-002**: 长期记忆（跨会话持久化，按租户隔离）
- [ ] **REQ-MEMORY-003**: 会话状态持久化与恢复，支持跨节点迁移
- [ ] **REQ-SKILL-001**: 声明式 Skill 格式（结构化骨架 + 自然语言指令，Markdown/YAML）
- [ ] **REQ-SKILL-002**: Skill 热加载、卸载和运行时切换
- [ ] **REQ-SKILL-003**: 交互式 Skill 创建（对话引导 → 意图提取 → 草稿生成 → Wasm 沙箱预览 → 迭代）
- [ ] **REQ-SKILL-004**: Skill 版本管理和持续迭代
- [ ] **REQ-SKILL-005**: Skill 组合为工作流 DAG，通过宿主消息总线传递
- [ ] **REQ-HITL-001**: 可配置审批策略（从不/写操作/高置信度/始终确认）
- [ ] **REQ-HITL-002**: 执行过程可随时暂停、修改方向或终止
- [ ] **REQ-SECURITY-001**: Wasm 多租户实例隔离（独立 Memory/Table 对象，线性内存硬边界）
- [ ] **REQ-SECURITY-002**: 工具执行沙箱（user namespace + chroot + seccomp + 路径标准化校验）
- [ ] **REQ-SECURITY-003**: 能力白名单（默认拒绝，显式授权）
- [ ] **REQ-SECURITY-004**: 网络访问控制（白名单域名 + SSRF 防护）
- [ ] **REQ-SECURITY-005**: 三层认证授权（接入层 API Key/JWT → 会话层 Session Token → 操作层运行时检查）
- [ ] **REQ-OBSERVABILITY-001**: 全链路追踪（session_id + instance_id 关联）
- [ ] **REQ-OBSERVABILITY-002**: 审计日志（全量记录 + 参数脱敏 + 异常检测）
- [ ] **REQ-OBSERVABILITY-003**: 监控指标暴露（实例数/资源使用/安全事件/性能指标）
- [ ] **REQ-DEPLOY-001**: 单机模式（<1000 并发实例）
- [ ] **REQ-DEPLOY-002**: 集群模式（多宿主节点 + Redis + 对象存储，水平扩展）
- [ ] **REQ-DEPLOY-003**: 个人→企业发布（Skill 打包发布为多租户服务，数据隔离）
- [ ] **REQ-DEPLOY-004**: 定时触发（cron）和事件触发（webhook）
- [ ] **REQ-UI-001**: 内置 Web Chat 界面，流式对话，统一本地和远程 UI
- [ ] **REQ-UI-002**: Skill 管理界面（创建/编辑/预览/测试/发布），对话式创建 + 表单式精炼
- [ ] **REQ-UI-003**: Skill 市场（发现/安装/评价/分享），Git-based 版本管理

### Out of Scope

- 移动端原生 App（Web-first，移动端后期考虑）
- 可视化拖拽工作流编辑器（v1 采用自然语言 + 表单式，后期可加）
- 多语言 SDK（MVP 聚焦 Rust 宿主 + Web UI）
- 第三方插件市场运营（先做 Git-based 分发，不做中心化市场）

## Context

项目处于从零开始的 greenfield 阶段。前期已完成：
- 技术架构设计文档（docs/jadepaw_discussion.md）：Wasm 运行时 + Rust 宿主 + 预初始化池 + 能力安全模型
- 脑暴讨论（.planning/notes/mvp-core-decisions.md）：MVP 形态、Agent Loop、Skill 自举等核心决策
- 功能需求清单（.planning/REQUIREMENTS.md）：7 大领域 28 条需求
- 待研究问题（.planning/research/questions.md）：7 个关键技术问题

参考产品：Claude Code（skill 系统）、OpenClaw（插件 SDK 设计）、GPT Builder（对话式 skill 创建）。差异化在于打通"个人创作 → 企业级多租户服务"的完整闭环。

技术环境：Rust 2024 edition + wasmtime + tokio + axum（Web 框架候选）。前端待定（原生 JS / HTMX / 轻量框架）。

## Constraints

- **Tech stack**: Rust + wasmtime + tokio。不可变更的核心组合
- **Isolation**: Wasm 线性内存模型提供硬件级隔离，不允许退化为进程级隔离
- **Deployment density**: 单机 ≥10000 活跃实例，冷启动 ≤5ms P99
- **Multi-tenancy**: 从 Day 1 就设计为多租户架构，不能后期打补丁
- **Interface**: 内置 Web 服务器统一本地和远程 UI，不做单独的 CLI 或桌面 App
- **License**: 开源项目，周期自由，质量优先于速度

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Wasm + Rust + tokio 技术栈 | 线性内存隔离 + 编译时安全 + 高性能异步 | Validated — Phase 1, 2 |
| wasmtime 45.0 (实际使用版本) | wasmtime 38 升级至 45.0：PoolingAllocationConfig API, async always-on, table_growing required, epoch_deadline 返回 () | Validated — Phase 2 |
| 预初始化实例池 + 懒加载 | Arc\<InstancePre\> + Store::new per session + Semaphore；冷启动 ~0-2ms avg | Validated — Phase 2 |
| 混合 Agent Loop（规划 + ReAct） | 目标导向性 + 执行灵活性，行为更像有计划的机器人 | — Pending |
| 内置 Web 服务器统一 UI | 同一套 UI 覆盖本地使用和远程企业部署 | — Pending |
| Skill 即自然语言程序 | 非开发者也能定制 Agent，降低使用门槛 | — Pending |
| 交互式 Skill 创建（自举） | Agent 的第一个内置 Skill 就是帮用户写 Skill | — Pending |
| MCP 兼容工具协议 | 与主流 Agent 生态互通，降低工具开发成本 | — Pending |
| 能力安全模型（默认拒绝） | InstanceCapabilities default deny, SessionState can_* methods, path sandbox validation | Validated — Phase 2 |
| Delegating Chain ResourceLimiter | InstanceHardLimiter (64MB Err) + TenantQuotaLimiter (budget Ok(false)), 可扩展组合 | Validated — Phase 2 |
| Fuel + Epoch 双计量 | InstanceHardLimiter 设 1M fuel, epoch ticker ~1ms interval, 双保险防资源滥用 | Validated — Phase 2 |
| HostFunctions trait (async_trait) | jadepaw-core 中定义，additive-only 设计，Phase 4 扩展 http_request | Validated — Phase 2 |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-05-30 after Phase 2*