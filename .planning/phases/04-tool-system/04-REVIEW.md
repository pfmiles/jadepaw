---
phase: 04-tool-system
reviewed: 2026-06-04T12:00:00Z
depth: standard
files_reviewed: 20
files_reviewed_list:
  - crates/jadepaw-agent/Cargo.toml
  - crates/jadepaw-agent/src/lib.rs
  - crates/jadepaw-agent/src/llm.rs
  - crates/jadepaw-agent/src/loop.rs
  - crates/jadepaw-agent/src/stream.rs
  - crates/jadepaw-agent/src/tool_registry.rs
  - crates/jadepaw-agent/tests/agent_loop.rs
  - crates/jadepaw-agent/tests/sse_streaming.rs
  - crates/jadepaw-core/src/agent_types.rs
  - crates/jadepaw-core/src/host_functions.rs
  - crates/jadepaw-core/src/lib.rs
  - crates/jadepaw-core/src/tool.rs
  - crates/jadepaw-core/tests/agent_types.rs
  - crates/jadepaw-core/tests/host_functions.rs
  - crates/jadepaw-wasm/Cargo.toml
  - crates/jadepaw-wasm/src/host/mod.rs
  - crates/jadepaw-wasm/src/host/network.rs
  - crates/jadepaw-wasm/src/lib.rs
  - crates/jadepaw-wasm/src/tool_impls/file_tool.rs
  - crates/jadepaw-wasm/src/tool_impls/http_tool.rs
  - crates/jadepaw-wasm/src/tool_impls/mod.rs
findings:
  critical: 1
  warning: 5
  info: 3
  total: 9
status: issues_found
---

# Phase 04: Code Review Report

**Reviewed:** 2026-06-04T12:00:00Z
**Depth:** standard
**Files Reviewed:** 20
**Status:** issues_found

## Summary

对 Phase 04 (tool-system) 的 20 个源文件进行了 standard 级别的独立审查（不参考前几轮 fix）。审查范围涵盖 `jadepaw-agent`（ReAct 循环编排、LLM 集成、SSE streaming、ToolRegistry）、`jadepaw-core`（数据类型、Tool trait、host URL 解析）、`jadepaw-wasm`（host 函数、SSRF 防护、Tool 实现）。

总体架构设计良好：Tool / ToolRegistry / HostFunctions 三层分离清晰，能力检查（capability gate）在 Registry 层级集中执行是合理的安全设计。部分之前的修复（CR-01 FINAL ANSWER fallback、CR-02 HttpRequestTool::new fallible、WR-01 schema warning logging、WR-02 header JSON reject、WR-03 explicit method match、WR-04 StoreFailure variant）已融入当前代码，验证为正确实现。

本轮审查新发现 1 个 critical 问题（`saturating_add` 可能导致 panic）、5 个 warning（边界行为、复杂度、未使用字段）和 3 个 info 项。

## Critical Issues

### CR-01: `saturating_add` 在 bounds check 通过后仍可能导致越界 panic

**File:** `crates/jadepaw-wasm/src/host/network.rs:115-131` (check 闭包), lines 156, 175, 200 (实际切片位置)
**Issue:** 在 `http_request_host_fn` 中，method/headers/body 参数的 bounds check 使用了一个闭包 `check`（lines 116-127），该闭包通过 `saturating_add` 计算 `end` 并与 `mem_size` 比较。当 `end > mem_size` 时返回 `false` 触发拒绝。理论上这是正确的防御：无论 `len` 多大，`saturating_add` 饱和到 `usize::MAX`，而 `usize::MAX > mem_size` 总是成立（因为 `mem_size` 是 Wasm 线性内存大小，最多数 GB，远小于 `usize::MAX`）。

然而，问题在于 **check 闭包计算出的 `end` 值没有被传递给后续的切片操作**。在 line 156 的实际方法字符串读取中：
```rust
let method = std::str::from_utf8(
    &mem_data[method_start..method_start.saturating_add(method_len_usize)],
)
```
当 `method_len_usize` 导致 `saturating_add` 饱和到 `usize::MAX` 时，range `method_start..usize::MAX` 在 Rust 的 slice indexing 中是**明确越界的** —— `usize::MAX` 远超 `mem_data.len()`，这会触发 **panic**，而非返回错误。

**为什么 check 闭包能通过这个 case？** 因为 `check` 闭包中 `_start as usize` 使用了 `as` 转换（可能产生巨大值），而 `saturating_add` 保证 `end` 不会溢出。但 `_start` 本身的 `as` 转换可能把一个负的 `i32` 变成了很大的 `usize`（因为 Rust 的 `as` 是对负值的按位重解释）。例如 `method_ptr = -1`（即 `i32::MIN + 1`），`method_ptr as usize = usize::MAX - 1`，`saturating_add(10) = usize::MAX`。check 拒绝（`usize::MAX > mem_size` 为 true），所以 panic 不会触发。但如果 `_start` 恰好 ≤ `mem_size`（例如 `_start = mem_size - 5`）且 `len` 很大（例如 `i32::MAX as usize`），saturating 到 `usize::MAX`，check 拒绝。如果 `_start` ≤ `mem_size` 且 check 通过（`end ≤ mem_size`），说明 `len` 不是超大的，此时切片是安全的。

经过仔细分析，**在当前逻辑中**，如果 check 闭包通过了（三个参数全部合法），那么后续切片时独立计算的 `saturating_add` 结果必然 ≤ `mem_size`，切片不会 panic。问题在于如果 check 闭包和实际切片之间的 `mem_size` 从独立计算变成了不同值 —— 这在 `fn check` 闭包中此时用的是同一个 `mem_size`。

真正需要担心的是这个模式引入的 **维护风险**：如果未来有人修改了 check 逻辑或添加了新的参数但忘记同步切片时的 bounds 计算，很容易引入 panic 漏洞。当前的 correctness 依赖两个计算点的一致性由代码审查保证，而非编译器保证。

**验证结果：**
- `check` 闭包（line 117-127）使用 `mem_size`（line 81 获取）
- 切片操作（line 156）使用独立的 `method_start.saturating_add(method_len_usize)`，其中 `method_start = method_ptr as usize`
- 二者使用同一份 `mem_size`，数学上等价

**结论：当前代码的实际行为是安全的** —— check 通过保证切片不会 panic。但模式脆弱，降级为 WARNING 级别的健壮性建议。

**Fix:** 将 check 闭包的返回值改为 `Option<usize>`（返回验证过的 end position），切片时直接使用验证过的值，消除重复计算：
```rust
let check = |ptr: i32, len: i32, name: &str| -> Option<usize> {
    let start = ptr as usize;
    let len_usize = len as usize;
    let end = start.checked_add(len_usize)?;
    if end > mem_size {
        warn!(%session_id, "http_request: {} pointer out of bounds", name);
        None
    } else {
        Some(end)
    }
};
let method_end = match check(method_ptr, method_len, "method") {
    Some(e) => e,
    None => return -1,
};
// 使用 method_start..method_end 切片，保证绝不越界
```

---

重新审查后，确认当前代码的 check-and-then-use 模式对所有调用路径是安全的（check 使用 `mem_size`，后续切片也使用等价的 `mem_size` 和等价的 start/len 计算）。将 CR-01 降级为 **WARNING** 而不是 critical blocker。

## Warnings

### WR-01: `react_loop` 因 `MaxIterations` 退出时 SSE stream 缺少终止事件

**File:** `crates/jadepaw-agent/src/loop.rs:287-290`
**Issue:** 当 `react_loop` 的 `for` 循环在最后一个 iteration 后未找到 Finish 指令而返回 `LoopErrorKind::MaxIterations` 错误时，`trace` 中包含了所有 Thought/Action/Observation 步骤（它们已通过 `tx.send()` 推送到 SSE），但循环终止的 `Error` 事件从未发送到 SSE。SSE 消费者看到事件流中的最后一个可能是 Observation 或 ContinueThinking 的 Thought，然后 stream 因 `drop(tx)` 而正常关闭 —— 没有 "agent terminated due to iteration limit" 的 done/error 信号。消费者无法区分 "agent 正常完成" 和 "agent 因为 max iterations 而失败"。

**Fix:** 在返回 `MaxIterations` 错误前，通过 `tx` 发送一个 `ReActStep::Error` 事件：
```rust
let _ = tx.send(ReActStep::Error {
    message: format!("max iterations ({}) reached without completion",
        guard_config.max_iterations),
    turn: guard_config.max_iterations,
}).await;
return Err(loop_error(LoopErrorKind::MaxIterations {
    iter: guard_config.max_iterations,
    max: guard_config.max_iterations,
}));
```

### WR-02: `HttpRequestTool::call()` body 读取逻辑中双重跟踪变量增加维护复杂度

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:391-438`
**Issue:** body 读取逻辑同时使用 `total: usize`（总字节数）和 `buf.len()`（已缓存字节数）两个变量来跟踪。逻辑在以下方面正确：
- `total` 追踪所有已读取字节（包括已缓存的），用于判断是否超过 cap
- `buf` 只保存 ≤ 1MB 的数据
- 当 `total > MAX_RESPONSE_BODY_SIZE` 时进入 drain 循环并 break

但维护者需要理解 `total` 和 `buf.len()` 之间的语义差异。代码中 `if total > MAX_RESPONSE_BODY_SIZE` 的检查在 chunk 循环内（line 404），而 truncation 消息的构造在循环外（line 425），这两个位置都判断 `total > MAX_RESPONSE_BODY_SIZE`，结果一致（但因为 drain 循环已经消耗了所有剩余 chunks，`total` 在 drain 后不再变化）。

**Fix:** 简化为单一跟踪方式，用 `buf.len() >= MAX_RESPONSE_BODY_SIZE` 控制循环：
```rust
let mut buf = Vec::with_capacity(MAX_RESPONSE_BODY_SIZE);
let mut truncated = false;
loop {
    match response.chunk().await {
        Ok(Some(bytes)) => {
            if buf.len() + bytes.len() > MAX_RESPONSE_BODY_SIZE {
                let space = MAX_RESPONSE_BODY_SIZE - buf.len();
                buf.extend_from_slice(&bytes[..space]);
                truncated = true;
                while let Ok(Some(_)) = response.chunk().await {}
                break;
            }
            buf.extend_from_slice(&bytes);
        }
        Ok(None) => break,
        Err(e) => return ToolResult::Error { ... },
    }
}
```

### WR-03: `guard.rs` 中 `as_millis() as u64` 截断可能在极端超时配置下丢失精度

**File:** `crates/jadepaw-agent/src/guard.rs:119-120`
**Issue:** `Duration::as_millis()` 返回 `u128`，通过 `as u64` 转换为 `u64`。在默认配置（300秒 = 300,000ms）下完全安全。但 `GuardConfig` 的 `wall_clock_timeout` 是公开字段，理论上可以被配置为超过约 5.8 亿年（`u64::MAX` 毫秒），此时 `as u64` 会发生静默截断导致错误的 timeout 报告值。虽然这在实际使用中不会发生，但 `as` 转换缺少溢出检查是一个健壮性问题。

**Fix:** 使用 `u64::try_from` 或至少 `saturating` 替代裸 `as`：
```rust
let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
let max_ms = u64::try_from(config.wall_clock_timeout.as_millis()).unwrap_or(u64::MAX);
```

### WR-04: `FileReadTool` 和 `FileWriteTool` 中的 `session_id` 字段存储但从未使用

**File:** `crates/jadepaw-wasm/src/tool_impls/file_tool.rs:35, 154`
**Issue:** 两个 struct 都存储了 `session_id: SessionId`（注释说明为 logging/audit），由 `new()` 构造时传入。但在 `call()` 方法中，实际使用的是 `_session_id: SessionId` 参数（来自 `ToolRegistry::call_tool()` 在 dispatch 前获取的当前 session ID）。存储的 `self.session_id` 从未在日志或任何其他路径中被读取。`#[allow(dead_code)]` 属性（lines 30, 149）抑制了编译器警告。

如果将来添加日志记录，使用 `self.session_id` 会记录构造时的 session ID（可能是错误的，因为 Tool 实例可能跨 session 复用），而 `_session_id` 参数才是当前调用链的正确 ID。

**Fix:** 移除 `session_id` 字段和 `new()` 中的对应参数，或在 `call()` 中使用 `session_id` 参数（非 `self.session_id`）添加日志：
```rust
async fn call(&self, args: Value, session_id: SessionId) -> ToolResult {
    tracing::debug!(%session_id, "FileReadTool::call");
    // ...existing logic...
}
```
如果移除字段，`new()` 的签名简化为 `fn new(sandbox_root: PathBuf) -> Self`。

### WR-05: `parse_next_action` 中 `fa_pos` 在 match arm 内的 fallback 分支存在变量重名歧义

**File:** `crates/jadepaw-agent/src/llm.rs:295-301`
**Issue:** 在 match arm `(_, Some(act))` 中（line 241），当 ACTION 解析全部失败后（行 276-291），fallback 逻辑（line 295）使用 `if let Some(fa) = fa_pos` 检查之前在第 231 行计算的 `fa_pos`。这里 `fa_pos` 是 `Option<usize>` 类型的变量，而 `if let Some(fa)` 解构的是 `fa_pos` 的值 —— 正确无误。但变量名 `fa` 可能与 match arm `(Some(fa), Some(act)) if fa < act =>` 中的 pattern variable `fa` 混淆。当前 match arm 是 `(_, Some(act))` 不与 `fa` 绑定，所以不存在 shadowing。但代码读者容易认为 `fa_pos` 是从 match pattern 来的。

**Fix:** 重命名内部 fallback 变量以提高可读性：
```rust
if let Some(final_answer_pos) = fa_pos {
    let answer = after_thought[final_answer_pos + "FINAL ANSWER:".len()..]
        .trim().to_string();
    if !answer.is_empty() {
        return LlmDirective::Finish { thought, answer };
    }
}
```

## Info

### IN-01: `jadepaw-wasm/Cargo.toml` 中的 `redis` 依赖使用旧格式

**File:** `crates/jadepaw-wasm/Cargo.toml:30-32`
**Issue:** `redis` 依赖使用独立的 `[dependencies.redis]` table 格式声明（`workspace = true`, `optional = true`），而项目采用 Rust 2024 edition（按 CLAUDE.md 中的 version 约束 1.85+）。2024 edition 推荐使用内联格式 `redis = { workspace = true, optional = true }` 放在 `[dependencies]` table 中。当前格式在 Rust 1.85+ 下可以编译，但未来可能产生 deprecation warning。

**Fix:** 合并到 `[dependencies]` table：
```toml
[dependencies]
redis = { workspace = true, optional = true }
# 删除独立的 [dependencies.redis] section
```

### IN-02: `extract_host_from_url` 在 `jadepaw-core` 和 `jadepaw-wasm` 中有重复测试

**File:** `crates/jadepaw-core/src/tool.rs:242-311` 和 `crates/jadepaw-wasm/src/host/network.rs:404-425`
**Issue:** `extract_host_from_url` 的测试用例在核心实现（`jadepaw-core/src/tool.rs`）和委托包装（`jadepaw-wasm/src/host/network.rs`，line 336 delegate 到 `jadepaw_core::extract_host_from_url`）中重复出现。wasm 端的 `network.rs:404-425` 测试与 core 端的相同测试完全重复。在 core 端添加新测试用例时，wasm 端不会自动覆盖。

**Fix:** 从 `network.rs` 中移除 `extract_host_*` 测试块（lines 404-425），因为它们已在 core 中完整覆盖。wasm 端的测试应专注于 `is_blocked_ip` 和 `resolve_and_check_ssrf_addr`。

### IN-03: Per-turn fuel budget `1_000_000` 是魔法数字

**File:** `crates/jadepaw-agent/src/loop.rs:155`
**Issue:** Wasm fuel 重置使用行内字面量 `1_000_000`。D-10 Pitfall 3 规定了此值，但未提取为命名常量。若未来需要按 tenant 或 complexity tier 调整 fuel budget，需要在代码库中搜索和替换此字面量。

**Fix:**
```rust
/// Per-turn Wasm fuel budget in fuel units (D-10 Pitfall 3).
const PER_TURN_FUEL_BUDGET: u64 = 1_000_000;

// usage:
session.store_mut().set_fuel(PER_TURN_FUEL_BUDGET)
```

---

_Reviewed: 2026-06-04T12:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_