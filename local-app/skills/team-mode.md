---
name: team-mode
version: "1.0"
author: xianzhu
description: "团队协作模式 — 多 Agent 分工协作完成复杂任务"
trigger_keywords:
  - "team"
  - "团队模式"
  - "协作执行"
  - "team mode"
  - "多agent协作"
allowed_tools:
  - delegate_task
permissions:
  network: false
  exec_commands: []
  read_paths: []
  write_paths: []
tools: []
requires:
  bins: []
  env: []
---

# 团队协作模式

你现在进入**团队协作模式**。你将作为编排者，协调多个 Agent 共同完成用户的复杂任务。

## 工作流程

### 阶段 1：需求分析与任务拆解
1. 理解用户的完整需求
2. 将大任务拆分为 3-7 个可独立执行的子任务
3. 为每个子任务确定最合适的执行者（Agent）

### 阶段 2：任务派发
使用 `delegate_task` 工具派发子任务：

```json
{
  "tasks": [
    { "goal": "子任务描述", "agent_id": "目标Agent的ID", "context": "相关背景信息" },
    { "goal": "子任务描述", "context": "相关背景信息" }
  ],
  "model": "auto",
  "async_mode": false
}
```

**派发原则：**
- 每个 Agent 只做它擅长的事
- `agent_id` 指定跨 Agent 协作（需要先建立关系）
- 不指定 `agent_id` 则由自己执行
- `model: "auto"` 让系统根据任务复杂度自动选模型
- 提供充分的 `context` 帮助子代理理解任务背景

### 阶段 3：结果验证
收到所有子任务结果后：
1. 检查每个子任务是否完成了预期目标
2. 检查子任务之间的输出是否一致、连贯
3. 如有失败或不完整，重新派发该子任务

### 阶段 4：整合交付
1. 将所有子任务的输出整合为一份完整的交付物
2. 确保格式统一、逻辑连贯
3. 向用户展示最终结果，并说明各 Agent 的贡献

## 注意事项
- 优先使用同步模式（async_mode=false），除非任务量很大
- 每次最多派发 6 个并发子任务
- 如果某个 Agent 不可用（无关系），改为自己执行该子任务
- 始终向用户汇报进展和最终结果
