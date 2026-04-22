# Character Eval 人格一致性评估

## 能力
系统 SHALL 支持评估 Agent 的回复是否忠实于人格设定，检测 persona drift。

## 需求

### Requirement: JSON 结构输出
评估 LLM SHALL 返回 `{"score": 0.0-1.0, "notes": "..."}`。

#### Scenario: 正常评估
- **WHEN** `/character` 或 `evaluate_character` 被调用
- **THEN** 抽样最近 48 小时的 user+assistant 消息（最多 30 轮）
- **AND** 返回 `CharacterEvalResult { consistency_score, drift_notes, sampled_turns, evaluated_at }`

#### Scenario: 样本不足
- **WHEN** 对话少于 4 轮
- **THEN** 返回错误"对话样本不足（需要至少 4 轮）"

### Requirement: 自动周评
系统 SHALL 每周日凌晨 4:00 为默认 Agent 运行人格评估。

### Requirement: 分数裁剪
`consistency_score` SHALL 被裁剪到 [0.0, 1.0]。
