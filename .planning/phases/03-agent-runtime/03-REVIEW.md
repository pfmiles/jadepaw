---
phase: 03-agent-runtime
reviewed: 2026-06-01T00:00:00Z
depth: standard
files_reviewed: 13
files_reviewed_list:
  - crates/jadepaw-core/src/agent_types.rs
  - crates/jadepaw-core/src/guest_exports.rs
  - crates/jadepaw-core/src/error.rs
  - crates/jadepaw-core/src/lib.rs
  - crates/jadepaw-agent/src/loop.rs
  - crates/jadepaw-agent/src/guard.rs
  - crates/jadepaw-agent/src/lib.rs
  - crates/jadepaw-agent/src/llm.rs
  - crates/jadepaw-agent/src/stream.rs
  - crates/jadepaw-core/tests/agent_types.rs
  - crates/jadepaw-agent/tests/agent_loop.rs
  - crates/jadepaw-agent/tests/termination.rs
  - crates/jadepaw-agent/tests/sse_streaming.rs
findings:
  critical: 4
  warning: 5
  info: 3
  total: 12
status: issues_found
---

# Phase 3: Code Review Report

**Reviewed:** 2026-06-01
**Depth:** standard
**Files Reviewed:** 13
**Status:** issues_found

## Summary

审核了 Phase 3 agent-runtime 的全部 9 个源文件和 4 个测试文件。代码整体结构清晰，遵循了 PHASE PLAN 和 RESEARCH 文档中定义的设计决策。发现 4 个 Critical 问题（错误映射语义错误、NextAction 类型重复、Thought 事件语义冗余）、5 个 Warning（缺少 source() 实现、终止追溯丢失转弯号、unwrap_or_default 吞掉错误、上下文详情泄露错误信息）和 3 个 Info 项。

---

## Critical Issues

### CR-01: run_with_guard 将所有 loop 错误统一映射为 WasmTrap —— 语义错误

**File:** `crates/jadepaw-agent/src/guard.rs:57-66`
**Issue:**
`run_with_guard` 在 agent_loop future 返回错误时，**无条件**地映射为 `AgentTerminationReason::WasmTrap`。但 loop 内部存在多种错误原因：
- LLM API 调用失败（网络错误、API key 无效）
- `anyhow::bail!("max iterations ({}) reached without completion", ...)` —— 迭代耗尽
- channel 关闭（"output channel closed on turn {}"）
- fuel 设置失败

这些都不是 WasmTrap。调用者（包括 `run_agent`）无法区分"LLM API 挂了"和"Wasm 实例触发了 trap"，这会破坏错误处理链和工作流决策。

**Fix:**
`react_loop` 的返回类型目前是 `anyhow::Result<Vec<ReActStep>>`，所有错误路径都用 `anyhow` 传递，丢失了类型信息。应该将迭代耗尽单独返回为 `JadepawError::AgentTerminated(MaxIterationsReached)` 而不是 anyhow 字符串，并在 `run_with_guard` 中做更细致的分类：

```rust
// 在 run_with_guard 中，不应该将 anyhow error 一揽子映射为 WasmTrap
result = agent_loop() => {
    result.map_err(|e| {
        // 更合理的做法：检查 root cause 并映射到对应的 termination reason
        // 或保留原始错误信息而非强制归为 WasmTrap
        JadepawError::agent_terminated(
            AgentTerminationReason::WasmTrap {
                reason: e.to_string(),
                turn: 0,
            },
        )
    })
}
```

建议：`react_loop` 返回 `Result<Vec<ReActStep>, JadepawError>` 而非 `anyhow::Result`，让 loop 内部使用 `JadepawError::AgentTerminated(MaxIterationsReached { ... })` 表示迭代耗尽，用新的变体（或保持 anyhow 但通过 downcast 或 error chain 判断）表示 LLM 错误。

### CR-02: react_loop 每 turn 对整个 LLM 响应流都发 Thought 事件——单个 Thought 变成几百个 Thought

**File:** `crates/jadepaw-agent/src/llm.rs:130-133`, `crates/jadepaw-agent/src/loop.rs:86-93`
**Issue:**
`stream_llm_response` 将每个 token delta 都 emit 为 `ReActStep::Thought`：
```rust
let step = ReActStep::Thought {
    content: content.clone(),
};
if tx.send(step).await.is_err() { ... }
full_content.push_str(&content);
```

对于一个典型的 LLM 响应（可能包含 200-500 个 token），这会生成 200-500 个 `Thought` 事件。每个 token 长度可能只有 1-4 个字符，对 SSE 消费者来说完全不可用——这些应该合并为一个 `Thought`，而 token 级流应使用单独的 `token` 事件名（D-14 规范中定义了 `token` 作为事件类型，但 stream.rs 中未实现）。

**Fix:**
`stream_llm_response` 应该 emit token 作为 `ReActStep::Thought` 的实际语义是让每个 token 都是一个"思维步骤"，这语义上不合理。按 D-14 规范，有两种合理做法：

1. 让 `stream_llm_response` **仅累积** `full_content`，不发送任何事件，由 `react_loop` 在拿到完整响应后 emit 一个 `ReActStep::Thought`
2. 添加一个新的 `ReActStep` variant（如 `ReActStep::Token { content: String }`）用于 token 级流，并在 `stream.rs` 中映射为 `event: token`

当前实现会导致 SSE stream 中出现几百个 `event: thought`，消费者无法区分"这是一个完整的思维步骤"和"这是思维步骤的碎片 token"。

### CR-03: llm::NextAction 与 guest_exports::NextAction 重复定义

**File:** `crates/jadepaw-agent/src/llm.rs:49-66`, `crates/jadepaw-core/src/guest_exports.rs:38-54`
**Issue:**
`jadepaw-core` 中定义了 `guest_exports::NextAction`：
```rust
pub enum NextAction {
    ContinueThinking,
    Act { tool: String, args: serde_json::Value },
    Finish { answer: String },
}
```

`jadepaw-agent/src/llm.rs` 中又定义了一个私有的 `llm::NextAction`：
```rust
pub enum NextAction {
    Act { tool: String, args: String },
    Finish { answer: String },
    ContinueThinking,
}
```

两者的变体基本相同但 `args` 类型不一致（一个用 `serde_json::Value`，一个用 `String`）。`loop.rs` 使用 `crate::llm::NextAction`，`parse_next_action` 返回 `crate::llm::NextAction`。

这造成了两个语义重复但结构略有不同的类型——将来如果 `guest_exports::NextAction` 被用于实际 guest 决策点，就会出现类型不兼容的问题。

**Fix:**
选择一种方案：
1. 将 `llm::NextAction` 改为使用 `jadepaw_core::guest_exports::NextAction`，统一类型
2. 如果 llm.rs 中的 `NextAction` 确实是 LLM 解析专用（`args` 为 raw string），应将其重命名为 `ParsedAction` 或 `LlmDirective` 以避免与 core 中的类型混淆

### CR-04: parse_next_action 未解析完整的 LLM 响应结构——THOUGHT 内容丢失

**File:** `crates/jadepaw-agent/src/llm.rs:162-198`
**Issue:**
系统提示词明确要求 LLM 按以下格式输出：
```
THOUGHT: <reasoning>
ACTION: <tool_name>(<args>)
```
或
```
THOUGHT: <reasoning>
FINAL ANSWER: <answer>
```

但 `parse_next_action` 仅查找 `ACTION:` 和 `FINAL ANSWER:`，**完全不提取 THOUGHT 内容**。`react_loop` 中 `parse_next_action` 返回的三种动作（Act / Finish / ContinueThinking）没有一种保留 THOUGHT 内容。这导致执行追踪中丢失了思维推理内容——而 THOUGHT 是整个 ReAct 模式的核心。

**Fix:**
`NextAction` 的三种变体都应该包含 `thought: String` 字段来保留 LLM 的推理内容。`ContinueThinking` variant 尤其应该携带 thought 内容。否则 `react_loop` 中的 trace 将只有 Action/Observation 而没有 Thought：

```rust
pub enum NextAction {
    Act {
        thought: String,
        tool: String,
        args: String,
    },
    Finish {
        thought: String,
        answer: String,
    },
    ContinueThinking {
        thought: String,
    },
}
```

并在 `parse_next_action` 中同时解析 THOUGHT 前缀。

---

## Warnings

### WR-01: JadepawError 未实现 `std::error::Error::source()`

**File:** `crates/jadepaw-core/src/error.rs:99`
**Issue:**
`impl std::error::Error for JadepawError {}` 使用了默认实现，其中 `source()` 返回 `None`。`AgentTerminated` 包装了 `AgentTerminationReason`，后者可能源自一个底层错误（WasmTrap），但通过 `source()` 无法访问到任何 root cause。任何进行错误链遍历的上层代码（如 `anyhow` 的 `.context()` / `.chain()` / `tracing` 的 `?` 展开）都无法获取完整的错误链。

**Fix:**
至少为 `AgentTerminated` 变体实现 `source()`：

```rust
impl std::error::Error for JadepawError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}
```

更彻底的修复是让 `AgentTerminated` 携带一个 `Option<Box<dyn std::error::Error + Send + Sync>>` 作为原始错误。

### WR-02: run_with_guard 中映射错误时丢失 turn 信息

**File:** `crates/jadepaw-agent/src/guard.rs:57-66`
**Issue:**
当 loop future 返回 error 时，`run_with_guard` 将错误映射为 `WasmTrap { turn: 0, ... }` —— 注释明确写道 "approximate — loop-internal errors don't expose exact turn"。但 loop.rs 中的多个错误路径实际上都知道当前 turn 号：
- 第 93 行: `"LLM call failed on turn {}", turn`
- 第 106 行: `"output channel closed on turn {}", turn`

这些 turn 信息在 `e.to_string()` 中存在，但在类型系统中丢失。

**Fix:**
在 `run_with_guard` 中进行简单的解析：从错误消息中提取 turn 号，或在 `react_loop` 中将确切的 turn 通过更结构化的方式传递（如返回一个包含 turn 的结构化错误类型）。

### WR-03: run_agent 中 trace 缺少 Finished 值时 unwrap_or_default 返回空字符串

**File:** `crates/jadepaw-agent/src/lib.rs:113-120`
**Issue:**
```rust
let final_answer = trace
    .iter()
    .rev()
    .find_map(|step| match step {
        ReActStep::Finished { answer } => Some(answer.clone()),
        _ => None,
    })
    .unwrap_or_default();
```

如果 trace 中没有 `Finished` step（例如被 guard 终止或所有迭代耗尽），`final_answer` 会是空字符串，而 `AgentResponse` 会正常返回但 `final_answer` 为空。调用者无法区分"Agent 正常完成并给出空回答"和"Agent 被终止但没有最终回答"。这是一个信息传递上的语义歧义。

**Fix:**
当找不到 `Finished` step 时，应返回一个错误而不是静默设为空字符串。或者将 `final_answer` 改为 `Option<String>`，让调用者自行判断。

### WR-04: SSE injection test 被弱化——仅验证内容存在但不验证安全性

**File:** `crates/jadepaw-agent/tests/sse_streaming.rs:156-189`
**Issue:**
测试 `test_sse_injection_sanitization` 的目的是验证 T-03-05（SSE 控制字符注入）：
```rust
let malicious_content = "\n\nevent: fake\ndata: injected\n\n";
```

但测试断言仅为：
```rust
assert!(dbg.contains("fake") || dbg.contains("injected"),
    "content should be present in event: {dbg}");
```

这个断言只验证了"内容在事件中存在"，但**没有验证注入未成功**。axum 的 `Event::data()` 方法确实会按行分割 `data:` 字段（这是合法的 SSE 多行数据），但这意味着 `\n\nevent: fake\ndata: injected\n\n` 作为 content 传递时，会被 axum 格式化为：
```
event: observation
data: 
data: event: fake
data: data: injected
data: 
data: 
```

这对 SSE 消费者来说**仍然是安全的**（因为 `event:` 出现时已经是 data 字段内容，不是新的 event 声明），但测试没有严格验证这一点——测试没有确认 "event: observation" 是唯一的 event 声名。

**Fix:**
对 SSE 流进行更严格的断言：确认只有 1 个事件，且该事件没有引入额外的 event 类型切换。可以检查 Debug 输出中 `event: observation` 出现**之后**没有再次出现 `event: ` 前缀：

```rust
// 确认没有伪造的 event 类型注入
let event_count = dbg.matches("event: ").count();
// 对原始格式的 Event Debug 输出，可能包含多个 data 行但不包含新的 event 行
// 更可靠的是用 serde/SSE 解析器验证流结构
```

### WR-05: react_loop 中 Action step 的 args JSON 解析使用 unwrap_or 作为兜底

**File:** `crates/jadepaw-agent/src/loop.rs:114-115`
**Issue:**
```rust
args: serde_json::from_str(&args)
    .unwrap_or(serde_json::Value::String(args.clone())),
```

如果 `llm::parse_next_action` 返回的 `args` 字符串不是合法的 JSON（例如 `location="Paris", unit="celsius"` —— 这是类 Python 语法而非 JSON），`serde_json::from_str` 会静默失败，回退为 `Value::String(...)`。这意味着下游消费者收到的 `args` 格式取决于 LLM 输出格式的质量，可能出现不可预期的类型（某个 turn 是 `Value::Object`，下个 turn 是 `Value::String`）。

**Fix:**
两种选择：
1. 始终将 args 保存为 `Value::String`（保持一致性，后续处理负责解析）
2. 在 `llm::parse_next_action` 中尝试 JSON 解析，如果失败则将错误信息记录到日志中，但仍返回 raw string

---

## Info

### IN-01: react_loop 对 ContinueThinking 未向 trace 添加 Thought step

**File:** `crates/jadepaw-agent/src/loop.rs:142-150`
**Issue:**
`NextAction::ContinueThinking` 分支不向 trace 添加任何 `ReActStep`：
```rust
NextAction::ContinueThinking => {
    let assistant_msg: ChatCompletionRequestMessage = ...;
    messages.push(assistant_msg);
}
```

对比 Act 分支会同时添加 Action 和 Observation。这意味着连续多轮的 "继续思考" 在 trace 中不留下任何记录，导致 trace 仅包含最终的 Action/Observation 和 Finished 步骤，缺少中间推理过程。

**Fix:**
即使没有采取行动，也应该记录 Thought 步骤：
```rust
NextAction::ContinueThinking { thought } => {
    let step = ReActStep::Thought { content: thought };
    if tx.send(step.clone()).await.is_err() {
        anyhow::bail!("output channel closed on turn {}", turn);
    }
    trace.push(step);
    // append assistant msg to history...
}
```

### IN-02: parse_next_action 在 ACTION: 后无括号时将整个字符串当作 tool name

**File:** `crates/jadepaw-agent/src/llm.rs:187-193`
**Issue:**
当 `ACTION:` 后面没有括号包裹参数时，代码将整行内容当作 tool name，args 为空：
```rust
let tool = action_str.trim().to_string();
if !tool.is_empty() {
    return NextAction::Act {
        tool,
        args: String::new(),
    };
}
```

这不是 bug——它是合理的降级处理——但这个行为没有文档说明，也可能与 LLM 的实际输出不匹配（例如 LLM 输出 `ACTION: get_weather location=Paris` 无括号）。建议添加注释说明这是 fallback 行为，并发出一个 warn 级别的日志。

### IN-03: llm.rs 中 parse_next_action 查找 ACTION: 时不以词边界匹配

**File:** `crates/jadepaw-agent/src/llm.rs:174`
**Issue:**
```rust
if let Some(pos) = response_upper.find("ACTION:") {
```

`find` 会在任意位置匹配，包括可能在非首字母的位置（例如 THOUGHT 内容中包含 "the ACTION: was to call..."）。虽然 `find` 返回第一个匹配位置，"FINAL ANSWER:" 先检查，但 THOUGHT 内容中的 "ACTION:" 不会被排除。

**Fix:**
如果 LLM 严格遵守提示词格式（`ACTION:` 出现在行首），可以使用更精确的匹配，或限制只在 `\nACTION:` 或字符串中单独的行上匹配。当前实现的风险较低（LLM 不太可能在 THOUGHT 中恰好写出 `ACTION:` 前缀），但防御性编程应考虑。

---

_Reviewed: 2026-06-01T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_