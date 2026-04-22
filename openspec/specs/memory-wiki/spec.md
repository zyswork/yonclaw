# Memory Wiki

## 能力
系统 SHALL 聚合多天 REM 记忆为单个 wiki 文件，作为 Agent 的"信念库"检索入口。

## 需求

### Requirement: 编译入口
系统 SHALL 提供 `compile_memory_wiki` Tauri 命令 + `/wiki [days]` 斜杠命令。

#### Scenario: 默认 14 天
- **WHEN** `/wiki`（无参）
- **THEN** 合并最近 14 天的 REM 文件
- **AND** 写入 `memory/dreaming/rem/WIKI.md`

#### Scenario: 超过 60 天被裁剪
- **WHEN** `/wiki 100`
- **THEN** 使用 60 天上限

### Requirement: 空目录处理
系统 SHALL 在 rem 目录不存在或无可用文件时返回明确错误，不 panic。
