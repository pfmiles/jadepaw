---
status: partial
phase: 06-skill-system
source: [06-VERIFICATION.md]
started: 2026-06-06T01:30:00Z
updated: 2026-06-06T01:30:00Z
---

## Current Test

[awaiting human testing]

## Tests

### 1. End-to-end skill loading and behavior change

- **Test:** 创建 SKILL.md 文件放入 skills 目录，通过 POST /skills/load 加载，发送对话请求，确认 Agent 行为发生变化
- **Expected:** Agent 的回复反映出 SKILL.md 中定义的技能行为（如 code-reviewer 风格审查代码）
- **Result:** [pending]

### 2. Mid-session skill swap

- **Test:** 会话中途通过 POST /skills/load 切换技能，继续同一会话发送消息，确认 Agent 行为在新回合切换
- **Expected:** 下一个 ReAct turn 开始后，Agent 行为切换到新技能（旧技能指令不再生效）
- **Result:** [pending]

### 3. Skill unload and behavior reversion

- **Test:** 通过 POST /skills/unload 卸载技能后继续对话，确认 Agent 恢复默认行为
- **Expected:** Agent 行为回退到无技能时的基础 ReAct 模式
- **Result:** [pending]

### 4. Error message quality for invalid SKILL.md

- **Test:** 提交格式错误的 SKILL.md（无效 YAML、缺少 name 字段、name 与目录名不匹配），观察 API 返回的错误信息
- **Expected:** API 返回 400 状态码，错误消息清晰指出具体问题（哪个字段、什么规则违反）
- **Result:** [pending]

### 5. Multi-skill merge behavior

- **Test:** 同时加载多个 SKILL.md 技能，确认 Agent 系统提示中正确合并了工具声明和指令
- **Expected:** 系统提示中包含所有已加载技能的 <skill_instructions> XML 块，工具列表去重合并
- **Result:** [pending]

### 6. Startup scan idempotency

- **Test:** 删除 SQLite 数据库文件后重启服务，确认索引重新构建正确
- **Expected:** walkdir 扫描重新发现所有 SKILL.md 文件并重建 skill_index 表
- **Result:** [pending]

## Summary

total: 6
passed: 0
issues: 0
pending: 6
skipped: 0
blocked: 0

## Gaps