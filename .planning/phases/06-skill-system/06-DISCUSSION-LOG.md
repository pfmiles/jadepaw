# Phase 6: Skill System - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-06-05
**Phase:** 06-skill-system
**Areas discussed:** SKILL.md 格式设计, Skill 上下文注入机制, 热加载/运行时切换, Skill 发现与存储

---

## SKILL.md 格式设计

| Option | Description | Selected |
|--------|-------------|----------|
| A: 严格 Agent Skills 标准 | 仅 name+description，其余进 metadata Map | |
| B: 标准 + 精选扩展 | Agent Skills 标准 + tools/constraints/version/author 顶层字段 | ✓ |
| C: Claude Code 格式 | 13+ 字段对齐 Claude Code SKILL.md | |
| D: jadepaw 专有格式 | 完全自定义字段对齐内部架构 | |

**User's choice:** B: Agent Skills 标准 + 精选扩展
**Notes:** 核心兼容 agentskills.io（未知字段被忽略），jadepaw 特定语义类型安全。扩展字段以 `x-jadepaw-` 为前缀避免未来标准冲突。

---

## Skill 上下文注入机制

| Option | Description | Selected |
|--------|-------------|----------|
| A: System Prompt 拼接 | 所有 skill 串联追加到 REACT_SYSTEM_PROMPT | |
| B: 独立 System Messages | 每条 skill 作为独立 system message | |
| C: User Message Prefix | 复用 context 字段，前置到用户消息 | |
| D+E 组合 | XML 结构化注入 + Late-Binding 动态重建 | ✓ |
| D Only | XML 注入 + 一次性缓存 | |

**User's choice:** D+E 组合
**Notes:** D 提供结构化可审计的注入格式，E 支持 mid-session skill swap 时动态重建 system prompt。多 skill 按 priority 排序，冲突采用 priority-based override。Tool 声明 union + conflict detection。

---

## 热加载/运行时切换

| Option | Description | Selected |
|--------|-------------|----------|
| A: API 驱动 | REST API load/unload，确定性状态 | ✓ |
| B: notify 文件监听 | inotify/FSEvents 自动检测文件变更 | |
| C: 周期性轮询 | tokio::interval stat mtime | |
| D: Hybrid API+Watch | API + file watch 共享事件通道 | |

**User's choice:** A: 纯 API 驱动
**Notes:** 对齐 jadepaw Web-first 架构。Mid-session swap 在当前 turn 完成后原子生效。验证失败拒绝并保留当前 skill。运行时状态在内存（Arc<DashMap>）。Phase 7+ 可扩展 Hybrid 模式。

---

## Skill 发现与存储

| Option | Description | Selected |
|--------|-------------|----------|
| A: 纯文件系统 | walkdir 扫描，无数据库 | |
| B: DB 主存储 | skill 定义存 SQLite，文件为备份 | |
| C: 文件 + DB 索引 | 文件 source of truth，SQLite 索引缓存 | ✓ |
| D: 分层 + DB 索引 | global/tenant 分层目录 + SQLite 索引 | |

**User's choice:** C: 文件主存储 + SQLite 索引缓存
**Notes:** `~/.jadepaw/skills/<tenant_id>/<skill_name>/SKILL.md` 为 source of truth。启动时 walkdir 扫描提取 metadata 写入 SQLite `skill_index` 表。API list 走 DB 索引，load 读文件。多租户通过目录隔离。

---

## Claude's Discretion

No areas were deferred to Claude — all decisions were user-directed.

## Deferred Ideas

- 交互式 Skill 创建 (SKILL-03) — v2
- Skill 版本管理和迭代 (SKILL-04) — v2
- Skill 工作流 DAG (SKILL-05) — v2
- 文件监听自动重载 (notify) — Phase 7+
- Git-based Skill 分发/市场 — v2
- Per-skill token budget — 后期优化
- Skill 间显式冲突解决 DSL — 后期优化