# Dreaming / REM 记忆整理

## 能力
系统 SHALL 定期从最近对话中提取观察并持久化，模拟人脑的睡眠记忆整理机制。

## 需求

### Requirement: Light Sleep 浅睡整理
系统 SHALL 每日凌晨 3:00 自动触发 Light Sleep，提取最近 24 小时对话中的观察性事实。

#### Scenario: 自动触发
- **WHEN** 到达配置的 Cron 时间
- **THEN** 对每个 Agent 拉取最近 24 小时消息，调用 LLM 按模板提取最多 5 条观察
- **AND** 写入 `~/.xianzhu/agents/{id}/memory/dreaming/light/YYYY-MM-DD.md`

#### Scenario: 手动触发
- **WHEN** 用户输入 `/dream light`
- **THEN** 调用 `run_dreaming` Tauri 命令，返回 `{phase, path, summary}`
- **AND** 斜杠命令在 chat 中显示摘要

### Requirement: REM Sleep 深度整理
系统 SHALL 每周日凌晨 3:30 触发 REM，从最近 3 天对话做模式 / 关联 / 信念层分析。

#### Scenario: 输出三小节
- **WHEN** REM 任务执行
- **THEN** 输出包含 `## Patterns` / `## Connections` / `## Beliefs`
- **AND** 写入 `memory/dreaming/rem/YYYY-MM-DD.md`

### Requirement: 空结果识别
系统 SHALL 精确匹配"本日无新观察"整行，不截掉合法输出。
