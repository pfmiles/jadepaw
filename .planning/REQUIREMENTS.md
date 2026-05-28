# Requirements: jadepaw

**Defined:** 2026-05-28
**Core Value:** 让任何人都能用自然语言"编程"自己的 AI Agent，并将它部署为可供成百上千人同时使用的企业级服务。

## v1 Requirements

Requirements for initial release. Each maps to roadmap phases.

### Agent Core

- [ ] **AGENT-01**: Agent 支持 ReAct 执行循环（think → tool → observe → next），根据自然语言输入自主选择和调用工具完成任务
- [ ] **AGENT-02**: 支持工具/函数调用，工具通过 MCP 兼容协议注册，MVP 至少支持文件读写和 HTTP 请求
- [ ] **AGENT-03**: 流式输出（token 级 SSE），用户实时看到 Agent 思考和执行进展
- [ ] **AGENT-04**: Agent 执行循环具备基础终止防护：最大迭代次数限制 + 超时限制

### Memory

- [ ] **MEM-01**: 单次会话内的对话上下文管理，支持上下文窗口管理和自动压缩
- [ ] **MEM-02**: 会话状态持久化（SQLite 单机模式），支持会话暂停和恢复

### Skill System

- [ ] **SKILL-01**: 声明式 Skill 格式——采用 Agent Skills 开放标准（SKILL.md: YAML frontmatter + Markdown 指令），可版本控制
- [ ] **SKILL-02**: Skill 热加载和运行时切换，Skill 作为 Agent 行为的持久化配置注入执行上下文

### Security & Isolation

- [ ] **SEC-01**: Wasm 实例隔离——每个会话运行在独立 wasmtime Store 中，线性内存提供硬件级隔离
- [ ] **SEC-02**: wasmtime Fuel + Epoch 双重资源计量从 Day 1 启用，显式 StoreLimits（64MB 内存上限）
- [ ] **SEC-03**: 工具执行通过宿主中介，路径参数强制标准化和沙箱边界检查
- [ ] **SEC-04**: 能力白名单——实例初始化时声明允许的工具和能力，默认拒绝

### User Interface

- [ ] **UI-01**: 内置 Web Chat 界面（HTMX + SSE），流式对话，统一本地和远程 UI
- [ ] **UI-02**: 基础 Skill 管理界面——Skill 列表查看、加载、卸载

### Observability

- [ ] **OBS-01**: 基础日志和追踪——每个操作关联 session_id 和 instance_id
- [ ] **OBS-02**: 关键指标暴露：活跃实例数、内存使用、响应时间（tracing + metrics + Prometheus exporter）

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Agent Intelligence

- **AGENT-05**: 混合 Agent Loop（粗粒度规划 + ReAct 执行），计划偏离时局部重规划
- **AGENT-06**: 多 Agent 编排（router → specialist 模式），子 Agent 独立工具集

### Creator Experience

- **SKILL-03**: 交互式 Skill 创建——对话引导 → 意图提取 → 草稿生成 → Wasm 沙箱预览 → 迭代精炼
- **SKILL-04**: Skill 版本管理和精炼迭代
- **SKILL-05**: Skill 组合为工作流 DAG，通过宿主消息总线传递
- **UI-03**: Skill 管理面板——对话式创建 + 表单式精炼的混合模式

### Memory & State

- **MEM-03**: 长期记忆——跨会话持久化，按租户隔离，关键信息自动提取和检索
- **MEM-04**: 会话跨节点迁移（Redis 集群）

### Enterprise Security

- **SEC-05**: 三层认证授权（API Key/JWT → Session Token → 运行时权限检查）
- **SEC-06**: 网络访问白名单域名限制 + SSRF 防护
- **SEC-07**: 工具执行三层沙箱（capability check → CLONE_NEWUSER + chroot → seccomp BPF）

### Enterprise Platform

- **DEPLOY-01**: 个人→企业发布——Skill 打包发布为多租户服务，数据隔离
- **DEPLOY-02**: 集群模式——多宿主节点 + Redis + 对象存储，水平扩展
- **DEPLOY-03**: 定时触发（cron）和事件触发（webhook）

### Observability

- **OBS-03**: 全链路审计日志——宿主函数调用全量记录 + 参数脱敏 + 异常检测
- **OBS-04**: 分布式追踪（tracing-opentelemetry）

### Ecosystem

- **UI-04**: Git-based Skill 市场——Skill 发现、安装、评价、分享

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| 混合 Agent Loop（v1） | 纯 ReAct 已验证足够（Claude Code 证明），混合规划增加 prompt 工程复杂度，待 v1 验证核心价值后再加 |
| 多 Agent 编排（v1） | 需要事件总线和 agent 通信基础设施，Phase 2 建设 |
| 长期记忆（v1） | 需要向量数据库和提取 pipeline，MVP 聚焦短期记忆 |
| 交互式 Skill 创建（v1） | 需要完整的 Wasm 沙箱预览机制，是 Phase 2 的"aha moment" |
| 企业级多租户路由（v1） | 架构从 Day 1 设计为多租户，但完整路由和发布管道在 Phase 3 |
| 可视化拖拽工作流编辑器 | NL + 表单式交互更符合"自然语言编程"理念，拖拽式后期评估 |
| 中心化 Skill 市场 | Git-based 分发更符合开源定位，避免市场运营负担 |
| 移动端原生 App | Web-first 策略，移动端后期考虑 PWA |
| 原生 Anthropic API 支持 | async-openai 的 OpenAI 兼容模式覆盖 90% 市场，原生支持按需加 |
| langchain-rust 集成 | 框架会与自定义 Agent Loop 冲突，直接使用 async-openai |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| AGENT-01 | — | Pending |
| AGENT-02 | — | Pending |
| AGENT-03 | — | Pending |
| AGENT-04 | — | Pending |
| MEM-01 | — | Pending |
| MEM-02 | — | Pending |
| SKILL-01 | — | Pending |
| SKILL-02 | — | Pending |
| SEC-01 | — | Pending |
| SEC-02 | — | Pending |
| SEC-03 | — | Pending |
| SEC-04 | — | Pending |
| UI-01 | — | Pending |
| UI-02 | — | Pending |
| OBS-01 | — | Pending |
| OBS-02 | — | Pending |

**Coverage:**
- v1 requirements: 16 total
- Mapped to phases: 0
- Unmapped: 16 ⚠️

---
*Requirements defined: 2026-05-28*
*Last updated: 2026-05-28 after research synthesis*