# Phase 5: Session Memory - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-06-04
**Phase:** 5-Session Memory
**Areas discussed:** Context Window Strategy, Persistence Schema & Storage, Pause/Resume Lifecycle, Session Isolation Guarantees

---

## Context Window Strategy

| Option | Description | Selected |
|--------|-------------|----------|
| Hybrid + fixed budget | 固定 token 预算（如 4K token），超出部分摘要旧轮次为结构化摘要，最近 N 轮保留原文 | |
| Hybrid + adaptive threshold | 基于模型上下文窗口百分比动态触发摘要。灵活但需要更多测试 | ✓ |
| Full summarization | 每次压缩时将全部历史摘要为一段文本。实现更简单但丢失 tool observation 细节 | |

**User's choice:** Hybrid + adaptive threshold，触发百分比定为 **65%**（用户基于使用其他 agent 的经验性阈值）
**Notes:** 65% 是用户从实际 agent 使用中得出的经验值，不是随意选择。这不是可覆盖的默认值——这是一个锁定决策。

---

## Persistence Schema & Storage

| Option | Description | Selected |
|--------|-------------|----------|
| Hybrid: metadata + JSON blob | Session 元数据规范化列 + messages/trace 存为 JSON 数组。新增 jadepaw-db crate | ✓ |
| Full normalized schema | sessions + messages + trace_steps 三表，FK 约束，完全可查询 | |
| SQLx types::Json with compile-time checks | 单表 + sqlx::types::Json<T>，编译时类型安全 | |

**User's choice:** Hybrid: metadata + JSON blob
**Notes:** 新增 `jadepaw-db` crate，SessionRepository trait，SQLx compile-time queries 用于元数据列，runtime queries 用于 JSON blob 列。Schema 可扩展——Phase 6/9 可通过 migration 增加规范化表。

---

## Pause/Resume Lifecycle

| Option | Description | Selected |
|--------|-------------|----------|
| Full-state snapshot + continuation | 每轮边界保存消息历史+trace+guard累加器到SQLite。Resume从下一轮继续 | ✓ |
| Idempotent replay | 只持久化消息历史。Resume时从第0轮重放全部LLM调用 | |
| Checkpoint at turn boundary only | 最简实现——每轮边界保存，但无crash恢复 | |

**User's choice:** Full-state snapshot + continuation
**Notes:** wasmtime Store 不可序列化是根本约束——所有方案都必须在 resume 时创建新 Store。飞行中的 LLM 调用在崩溃时丢失，接受为 MVP 级别的文档化限制。

---

## Session Isolation Guarantees

| Option | Description | Selected |
|--------|-------------|----------|
| Repository layer + WAL mode | SessionStore trait 强制 session_id/tenant_id 参数 + SQLite WAL 模式 | ✓ |
| Repository layer only | 仅类型系统强制，不加 WAL 模式 | |
| WAL mode only | 仅 WAL 模式，WHERE 子句靠代码纪律 | |

**User's choice:** Repository layer + WAL mode (Recommended)
**Notes:** Wasm Store-per-session 仍是主安全边界。数据库层是内部持久化——Repository 层提供编译期强制正确性，WAL 提供务实并发模型。pool 大小 3-5 connections。

---

## Claude's Discretion

No areas were deferred to Claude — all decisions were user-directed.

## Deferred Ideas

- 长期记忆 MEM-03（跨会话知识提取）— v2
- 集群模式会话迁移 MEM-04 — v2
- tiktoken-rs WASM blob 大小（~5MB）— 监控构建体积影响