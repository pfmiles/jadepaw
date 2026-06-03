# Phase 4: Tool System - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-06-03
**Phase:** 04-tool-system
**Areas discussed:** Tool abstraction & execution path, MCP compatibility scope, HTTP tool implementation depth, Tool error reporting format

---

## Tool Abstraction & Execution Path

| Option | Description | Selected |
|--------|-------------|----------|
| Registry + capability-aware dispatch | Tool trait in jadepaw-core, ToolRegistry in jadepaw-agent, can_call_tool() as sole capability gate | ✓ |
| Hybrid + thin Tool abstraction | Tool trait + Registry + Wasm host fn coexistence, dual registration paths | |
| Pure host-side ToolRegistry | Bypass Wasm guest entirely, registry-only dispatch | |
| Extend HostFunctions, delay Tool trait | Add tool_call/tool_list to HostFunctions, fastest path but Linker immutability → Phase 6 refactor | |

**User's choice:** Registry + capability-aware dispatch (Recommended)
**Follow-up:** User asked about the difference between "Hybrid + thin Tool abstraction" and "Registry + capability-aware dispatch." Key distinction explained: Hybrid has two registration paths (Linker + Registry) that must stay in sync; Registry-only has a single source of truth. In Hybrid, agent loop may dispatch through guest Wasm; in Registry, agent loop always dispatches through Registry. Guest's `select_tool` export is advisory in Registry mode, enforced in Hybrid mode.

---

## MCP Compatibility Scope

| Option | Description | Selected |
|--------|-------------|----------|
| MCP-compatible wire format (internal) | Internal tools/list + tools/call JSON-RPC handlers, MCP tool definition format, zero new deps | ✓ |
| Full MCP client (rmcp crate) | Connect to external MCP server subprocesses, true Claude Code interoperability | |
| MCP as tool definition format only | Reuse MCP's ToolDefinition struct shape only, no wire protocol | |

**User's choice:** MCP-compatible wire format (Recommended)
**Follow-up:** User asked whether MCP will remain a de facto standard. Confirmed: MCP is under open governance (Apache 2.0), has broad ecosystem adoption (OpenAI, Google, Microsoft), rmcp has 11M+ downloads, and major AI tools (Claude Code, Cursor, Windsurf) use it. Unlikely to become obsolete in the near term.

---

## HTTP Tool Implementation Depth

| Option | Description | Selected |
|--------|-------------|----------|
| Domain whitelist + IP-layer check | can_network_to (existing) + tokio::net::lookup_host pre-resolution + block private/loopback/link-local/multicast IPs via std::net | ✓ |
| Pre-resolve + IP validate + pin reqwest IP | Above + resolve_to_addrs() to lock reqwest to validated IP, closes DNS rebinding window | |
| reqwest + DNS resolution hook | hickory-resolver + reqwest::dns::Resolve trait impl, most thorough | |
| Minimal MVP | Domain whitelist only, no IP-layer check, simplest | |

**User's choice:** Domain whitelist + IP-layer check (Recommended)
**Follow-up:** User asked about security implications of skipping SSRF protection. Explained: without IP-layer checks, agent can be tricked into accessing cloud metadata endpoints (169.254.169.254), internal databases, and other tenants' services. DNS rebinding is a real but sophisticated attack — accepted as documented known risk for MVP.

---

## Tool Error Reporting Format

| Option | Description | Selected |
|--------|-------------|----------|
| Structured ToolResult + LLM-friendly format | ToolResult enum (Ok/Error + error_code + is_retryable), MCP two-tier error model mapping, Observation gets is_error field | ✓ |
| Pure MCP-compatible format | Strict CallToolResult { content, isError } format | |
| Pure structured Rust enum (non-MCP) | ToolResult enum only, no MCP format mapping | |
| Plain string embedding | Reuse current Observation { result: String }, simplest | |

**User's choice:** Structured ToolResult + LLM-friendly format (Recommended)

---

## Claude's Discretion

No areas were deferred to Claude — all decisions were user-directed.

## Deferred Ideas

- Full MCP client (rmcp) for external MCP server connections → Phase 6+
- DNS rebinding defense (hickory-resolver or IP pinning) → post-MVP security audit
- Per-tool rate limiting → future ResourceLimiter chain extension
- Tool output streaming → post-MVP
- Per-tool configurable timeouts → post-MVP