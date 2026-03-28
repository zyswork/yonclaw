---
name: deep-research
description: 深度研究工具。给定一个主题，自动进行多步搜索、分析和总结，生成结构化的研究报告。当用户要求"研究"、"调研"、"分析"某个话题时使用此技能。
trigger_keywords:
  - 研究
  - 调研
  - 分析报告
  - 深度分析
  - research
  - investigate
command: bash
args_template: [deep-research.sh, "{topic}"]
---

# Deep Research Skill

## 工作流程
1. 理解研究主题，分解为 3-5 个子问题
2. 对每个子问题进行网络搜索
3. 分析搜索结果，提取关键信息
4. 综合所有信息，生成结构化报告

## 输出格式
Markdown 格式的研究报告，包含：
- 摘要
- 各子话题的详细分析
- 数据/证据引用
- 结论和建议
