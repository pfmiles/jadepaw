---
title: "MVP核心设计决策"
date: 2026-05-28
context: "基于jadepaw_discussion.md的脑暴讨论，明确了MVP形态、Agent Loop设计、Skill自举机制等关键决策"
---

## 产品定位

jadepaw是一个**可直接被最终用户使用的通用Agent引擎**，核心理念是"Skill即自然语言程序"——用户无需编程能力，通过自然语言即可定制Agent行为。

与Claude Code/OpenClaw的根本差异：**个人创作 → 交互式精炼 → 一键发布为多租户企业服务**，打通了"个人Agent"到"企业级Agent平台"的完整闭环。

## MVP形态决策

**选择：HTTP API + 内置Web Chat**

- 基于内置Web服务器（Rust/tokio/axum），统一本地使用和远程企业部署的UI代码
- 本地场景：`jadepaw serve` → 浏览器打开 `localhost:PORT` 使用Web Chat
- 企业场景：同一套Web UI部署到服务器，多租户通过Session隔离

## Agent Loop设计决策

**选择：混合模式（粗粒度规划 + ReAct执行）**

- 第一阶段：LLM生成高层计划（3-7个阶段性步骤）
- 第二阶段：每个步骤内部用ReAct循环执行（think → tool → observe → next）
- 计划偏离时触发局部重规划，不推翻整个任务
- 用户可实时看到进度："步骤2/5：汇总飞书数据"

优势：结合了目标导向性和执行灵活性，行为更像"有计划的机器人"。

## Skill自举机制

**核心理念：交互式Skill创建 = Agent的第一项内置Skill**

流程：对话引导 → 提取意图 → 生成结构化Skill草稿 → Wasm沙箱安全预览 → 迭代精炼 → 发布/部署

关键差异点：Wasm沙箱允许用户在隔离环境中**安全预览**未完成的Skill，这是GPT Builder和Claude Code都不具备的能力。

## Skill格式

采用声明式Markdown/YAML结构（借鉴Claude Code + OpenClaw模式）：
- 结构化骨架：name, description, trigger, tools, constraints
- 逻辑部分：自然语言指令填充
- 可版本控制、可分享、机器可读

## MVP最小可用定义

"可用"标准：对话输入 → Agent Loop规划并执行 → 产生可观测结果

不必贪多求全——先跑通最小闭环，后续迭代持续丰富功能。