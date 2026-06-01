---
phase: 03-agent-runtime
plan: 02
subsystem: agent-runtime
tags: [async-openai, sse, streaming, reAct, tokio-mpsc, axum, ChatCompletionStream, Box<dyn Config>]

# Dependency graph
requires:
  - phase: 03-agent-runtime
    plan: 01
    provides: "ReAct loop skeleton, LoopConfig, LlmProvider trait, run_agent(), guard module"
provides:
  - "Real async-openai LLM integration with streaming via Client<Box<dyn Config>>"
  - "SSE event relay (mpsc channel -> ReceiverStream -> axum Sse) with 5 event types per D-14"
  - "Updated ReAct loop with real LLM calls, parse_next_action(), multi-turn support"
  - "New run_agent() returning (AgentResponse, SSE stream) for Phase 7 HTMX frontend"
affects: [03-server-streaming, 07-ui-integration, 04-tool-execution]
---

tech-stack:
  added: [axum (SSE Event), tokio-stream, futures]
  patterns:
    - "Client<Box<dyn Config>> direct usage — no LlmClient trait abstraction per D-05/D-06"
    - "SSE event naming per D-14: thought, action, observation, error, done"
    - "Streaming pipeline: ChatCompletionStream -> mpsc::channel(256) -> ReceiverStream -> axum Sse"

key-files:
  created:
    - "crates/jadepaw-agent/src/llm.rs — async-openai integration: REACT_SYSTEM_PROMPT, stream_llm_response(), parse_next_action()"
    - "crates/jadepaw-agent/src/stream.rs — SSE token relay: create_sse_channel()"
    - "crates/jadepaw-agent/tests/sse_streaming.rs — 6 SSE integration tests"
  modified:
    - "Cargo.toml — async-openai chat-completion feature enabled"
    - "crates/jadepaw-agent/Cargo.toml — added axum, tokio-stream, futures, serde_json deps"
    - "crates/jadepaw-agent/src/loop.rs — real LLM integration replaces mock LlmProvider"
    - "crates/jadepaw-agent/src/lib.rs — new run_agent() signature, module declarations, re-exports"
    - "crates/jadepaw-agent/tests/agent_loop.rs — updated to work with new API"
    - "crates/jadepaw-agent/tests/termination.rs — unchanged, continues to pass"

key-decisions:
  - "Used async-openai 0.40.2 create_stream() instead of create() with stream:true (API enforces this per RESEARCH.md)"
  - "axum 0.8.9 Event builder is used exclusively — never manually format SSE strings"
  - "LlmProvider trait removed entirely in favor of Client<Box<dyn Config>> per D-05/D-06"
  - "LoopConfig.model removed — model is now a function parameter for runtime provider dispatch"
  - "Placeholder observations for tool execution (Phase 4 gate) instead of blocking loop progress"
  - "mpsc channel capacity fixed at 256 per RESEARCH.md pitfall 2"

patterns-established:
  - "Streaming pipeline: ChatCompletionStream -> mpsc -> ReceiverStream -> axum Sse"
  - "ReActStep-to-SSE-Event mapping with axum Event builder"
  - "parse_next_action() minimal parser: case-insensitive FINAL ANSWER / ACTION directive detection"

requirements-completed: [AGENT-01, AGENT-03]

# Metrics
duration: 16min
completed: 2026-06-01
---

# Phase 3 Plan 2: LLM Streaming + SSE Event Relay Summary

**async-openai ChatCompletionStream wired into ReAct loop with named SSE events per D-14 and real-time token streaming through tokio mpsc channel**

## Performance

- **Duration:** 16 min
- **Started:** 2026-06-01T04:15:16Z
- **Completed:** 2026-06-01T04:31:48Z
- **Tasks:** 3
- **Files modified:** 7 (3 created, 4 modified)

## Accomplishments

- async-openai chat-completion feature enabled in workspace with streaming types (ChatCompletionResponseStream) correctly resolved
- Full LLM integration module (llm.rs) with REACT_SYSTEM_PROMPT, stream_llm_response(), build_initial_messages(), and parse_next_action() parser
- SSE event relay (stream.rs) mapping all 5 ReActStep variants to named SSE events: thought, action, observation, error, done
- ReAct loop rewritten to use real async-openai Client<Box<dyn Config>> with multi-turn think-act-observe cycle
- run_agent() returns (AgentResponse, SSE stream) tuple for Phase 7 HTMX frontend direct consumption
- 27 tests passing (13 unit + 14 integration) covering SSE event correctness, real-time streaming, injection safety, and channel backpressure

## Task Commits

1. **Task 1: Workspace config + async-openai LLM integration** — `8bdf2a2` (feat)
2. **Task 2: SSE event relay and loop integration with real LLM** — `2a5f5be` (feat)
3. **Task 3: Streaming integration tests** — `ca28728` (test)

## Files Created/Modified

- `Cargo.toml` — async-openai = { version = "0.40", features = ["chat-completion"] }
- `crates/jadepaw-agent/Cargo.toml` — added axum, tokio-stream, futures, serde_json deps
- `crates/jadepaw-agent/src/llm.rs` — async-openai integration with REACT_SYSTEM_PROMPT, stream_llm_response(), build_initial_messages(), parse_next_action(), 7 inline unit tests
- `crates/jadepaw-agent/src/stream.rs` — SSE token relay with create_sse_channel(), 5 inline unit tests covering all event types, real-time verification, and injection safety
- `crates/jadepaw-agent/src/loop.rs` — rewritten: LlmProvider trait removed, real Client<Box<dyn Config>> parameter, multi-turn ReAct with parse_next_action(), placeholder observations for tools (Phase 4 gate)
- `crates/jadepaw-agent/src/lib.rs` — updated run_agent() returns (AgentResponse, SSE stream), new module declarations and re-exports
- `crates/jadepaw-agent/tests/sse_streaming.rs` — 6 integration tests: event correctness, all variants, real-time delivery, done event format, injection sanitization, backpressure
- `crates/jadepaw-agent/tests/agent_loop.rs` — updated to remove LlmProvider references, new channel-based tests

## Decisions Made

- async-openai 0.40.2 create_stream() returns `StreamResponse<CreateChatCompletionStreamResponse>` which is `Pin<Box<dyn futures::Stream + Send>>` — works with `futures::StreamExt::next()`
- axum 0.8.9 Event has no public getter methods (builder pattern with private buffer). Tests use Debug representation for inspection
- LlmProvider trait removed entirely — Client<Box<dyn Config>> is used directly per D-05/D-06 (no trait abstraction needed)
- parse_next_action() is a minimal string-scanning parser. Sophisticated parsing (regex, structured extraction) deferred to future iteration
- Placeholder observations for tool calls are intentional — full tool execution arrives in Phase 4

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] axum dependency missing from jadepaw-agent Cargo.toml**
- **Found during:** Task 2 (stream.rs SSE Event type usage)
- **Issue:** stream.rs and lib.rs import axum types but jadepaw-agent didn't depend on axum
- **Fix:** Added `axum = { workspace = true }` to crates/jadepaw-agent/Cargo.toml
- **Files modified:** crates/jadepaw-agent/Cargo.toml
- **Verification:** cargo build -p jadepaw-agent succeeds
- **Committed in:** 2a5f5be (Task 2 commit)

**2. [Rule 3 - Blocking] axum 0.8.9 Event API mismatch in test assertions**
- **Found during:** Task 2 (stream.rs unit tests)
- **Issue:** axum 0.8.9 `Event` uses builder pattern (`.event(T)`, `.data(T)` return `Self`, not `&Self`), no getter methods. Tests called `.event()` and `.data()` with no arguments expecting return values
- **Fix:** Rewrote test assertions to use `format!("{:?}", event)` (Debug representation) for property inspection
- **Files modified:** crates/jadepaw-agent/src/stream.rs (test module)
- **Verification:** 5 stream.rs tests pass
- **Committed in:** 2a5f5be (Task 2 commit)

**3. [Rule 3 - Blocking] agent_loop.rs integration tests referenced removed LlmProvider**
- **Found during:** Task 2 (build verification)
- **Issue:** tests/agent_loop.rs imported `LlmProvider` which was removed from lib.rs re-exports
- **Fix:** Rewrote agent_loop tests to use channel-based patterns (create_sse_channel) instead of LlmProvider mocks
- **Files modified:** crates/jadepaw-agent/tests/agent_loop.rs
- **Verification:** 3 agent_loop tests pass
- **Committed in:** 2a5f5be (Task 2 commit)

**4. [Rule 1 - Bug] format string brackets in test assertions**
- **Found during:** Task 2 (stream.rs test compilation)
- **Issue:** `{names[0]}` in format strings causes parse error — `[` interpreted as format specifier syntax
- **Fix:** Used separate arguments (`"text: {}", names[0]`) instead of inline bracket expressions
- **Files modified:** crates/jadepaw-agent/src/stream.rs (test module)
- **Verification:** All 13 unit tests pass
- **Committed in:** 2a5f5be (Task 2 commit)

---

**Total deviations:** 4 auto-fixed (3 blocking, 1 bug)
**Impact on plan:** All auto-fixes were necessary for compilation and test correctness. No scope creep. API version differences (axum 0.8.9 vs expected) handled via test assertion adaptation.

## Issues Encountered

- **Worktree path safety (#3099):** Initial Task 1 file writes landed in the main repo instead of the worktree. Re-applied all edits using worktree-relative paths derived from `git rev-parse --show-toplevel`. No data loss — identified and corrected before commit.
- **async-openai 0.40.2 API surface:** The crate uses `ChatCompletionRequestSystemMessage::from(&str)` (works) and `ChatCompletionRequestUserMessage::from(&str)` (works) for message construction. `CreateChatCompletionRequestArgs` derive_builder follows the standard pattern.
- **axum 0.8.9 Event:** No `PartialEq`, no getter methods. Test assertions use Debug format. This is a design choice in axum — Event is purely a builder, not an inspected struct.

## Known Stubs

- `crates/jadepaw-agent/src/loop.rs:122`: Placeholder Observation for tool action — "Full tool execution is coming in Phase 4." This is intentional per the plan's gate on tool execution.

## Next Phase Readiness

- async-openai ChatCompletionStream streaming pipeline is structurally complete and type-checks
- SSE event relay produces all 5 event types per D-14 with correct axum Event builder usage
- run_agent() returns (AgentResponse, SSE stream) — ready for Phase 7 HTMX frontend to consume directly
- The mpsc channel (256 capacity) provides backpressure for the full streaming path
- Tool execution is intentionally stubbed (Phase 4 gate) — placeholder observations keep the loop progressing

---
*Phase: 03-agent-runtime*
*Completed: 2026-06-01*