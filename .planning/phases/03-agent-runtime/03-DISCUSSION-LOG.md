# Phase 3: Agent Runtime - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-06-01
**Phase:** 03-agent-runtime
**Areas discussed:** Loop 放置位置, LLM 集成方式, 终止保护策略, 调用接口设计

---

## Loop 放置位置

| Option | Description | Selected |
|--------|-------------|----------|
| A: 宿主侧编排 | 循环完全在 jadepaw-agent (Rust) 中，guest 被动接收推理结果，SSE 零开销 | |
| C: 混合模式 | 宿主主循环 + guest export 决策点（evaluate_step, select_tool, should_continue），默认回退 LLM | ✓ |
| B: Guest 侧执行 | 循环逻辑完全在 Wasm 内部，guest 通过 host functions 主动调用 LLM 和工具 | |

**User's choice:** 混合模式 (C)
**Notes:** 用户确认 JIT harness 未来愿景中 skill 生成的 Wasm 代码包含真正的执行逻辑（不只是工具调用），需要在主 agent loop 之外自行决定执行规划和流程。混合模式的 guest export 决策点正是这些自定义逻辑的注入点。Phase 3 定义接口骨架，Phase 4/6 逐步扩展。

---

## LLM 集成方式

| Option | Description | Selected |
|--------|-------------|----------|
| C: 直接使用，按需抽象 | Phase 3 直接用 async-openai Client<Box<dyn Config>>，等 Anthropic 需求出现时再抽 trait | ✓ |
| D: trait 在 agent 内 | LlmClient trait 定义在 jadepaw-agent，与 async-openai impl 同 crate | |
| B: trait 在 core | LlmClient trait 放在 jadepaw-core，和 HostFunctions 同级 | |

**User's choice:** 直接使用，按需抽象 (C)
**Notes:** async-openai 的 Box<dyn Config> 已覆盖所有 OpenAI 兼容 provider。遵循 Phase 2 "先 concrete 后 abstract" 的已验证路径。

---

## 终止保护策略

| Option | Description | Selected |
|--------|-------------|----------|
| A: 纯宿主守卫 | tokio::select! 同时监听 loop future + 迭代计数器 + wall-clock timeout，结构化错误返回 | ✓ |
| C: 双层 timeout | A + 每个 turn 包裹 tokio::time::timeout | |
| B: 双轨协作 | A + Wasm 层新增 AgentLoopLimiter | |

**User's choice:** 纯宿主守卫 (A)
**Notes:** 用户询问了"drop future 丢失执行上下文"对用户体验的影响。澄清：已完成的 turn 和执行轨迹完好无损，仅最后一个未完成的 turn 不被记录。用户看到的是清晰的终止消息 + 完整推理步骤，可以发送"继续"恢复。用户接受这个 trade-off。

---

## 调用接口设计

| Option | Description | Selected |
|--------|-------------|----------|
| A: 类型在 core | AgentRequest/Response/ReActStep 在 jadepaw-core，执行接口在 jadepaw-agent | ✓ |
| C: 直接函数接口 | async fn run_agent() 签名，不用 trait | |
| B: 全部在 agent | 所有类型和执行接口都在 jadepaw-agent 内 | |

**User's choice:** 类型在 core (A)
**Notes:** 遵循 Phase 2 HostFunctions 的先例——类型在 core（零依赖），实现在下游 crate。gateway/server 可引用类型而不依赖 agent。

---

## Claude's Discretion

No areas were deferred to Claude — all decisions were user-directed.

## Deferred Ideas

- 纯 Guest 侧 loop（完全 Wasm 自主）：推迟到 Phase 6 JIT harness 成熟后
- LlmClient trait 抽象：推迟到 Anthropic 或非 OpenAI 兼容 provider 成为硬需求
- Per-turn LLM/tool timeout：推迟到 MVP 后按需叠加