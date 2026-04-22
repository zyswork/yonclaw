# Tauri Commands API

本文档列出 XianZhu 前端可通过 `invoke(<name>, <args>)` 调用的所有后端命令。

> 自动从 `local-app/src/main.rs` 的 `tauri::generate_handler!` 宏清单整理。

## 约定

- 参数通过 `invoke` 的第二个对象传递，驼峰（Tauri 自动映射到 Rust snake_case）
- 异步返回 `Promise<T>`，Rust 端 `Result<T, String>`
- 所有错误转为 `string`，前端 `try/catch` 捕获

---

## 1. Providers / 模型配置

| 命令 | 说明 |
|------|------|
| `save_config(key, value)` | 写入通用设置项 |
| `get_config(key)` | 读取设置项 |
| `get_providers()` | 列出所有 provider（apiKey 脱敏） |
| `save_provider(provider)` | 新增/更新 provider |
| `delete_provider(id)` | 删除 provider |
| `get_api_status()` | 健康检查 |
| `test_provider_connection(apiType, apiKey, baseUrl?, model?)` | 测试 key 有效性 |

## 2. Agent 管理

| 命令 | 说明 |
|------|------|
| `create_agent(...)` | 创建 Agent |
| `list_agents()` | 列出全部 |
| `list_agents_with_stats()` | 含统计（会话数 / 渠道数） |
| `delete_agent(id)` | 删除 |
| `update_agent(...)` | 更新 |
| `get_agent_detail(id)` | 详情 |
| `ai_generate_agent_config(...)` | LLM 根据描述生成 agent 配置 |
| `clone_agent(sourceId, newName)` | 克隆 |
| `snapshot_agent(id)` | 保存快照 |
| `list_agent_snapshots(agentId)` | 列出快照 |
| `list_agent_templates()` | 内置模板 |
| `export_agent_bundle(id)` / `import_agent_bundle(...)` | 导入导出 |

## 3. Session / 会话

| 命令 | 说明 |
|------|------|
| `send_message(agentId, sessionId, message)` | 流式发送（主路径） |
| `send_chat_only(...)` | 非工具调用模式 |
| `stop_generation(sessionId)` | 取消流式 |
| `get_conversations` / `get_session_messages` / `load_structured_messages` | 读消息 |
| `clear_history(sessionId)` | 清空会话 |
| `create_session / list_sessions / rename_session / delete_session` | 会话 CRUD |
| `compact_session(agentId, sessionId)` | 结构化压缩 |
| `cleanup_system_sessions(agentId, keepDays)` | 清理系统会话 |
| `search_messages / search_all_messages` | 单会话 / 跨会话搜索 |
| `export_session_history(sessionId, format)` | 导出会话 |
| `edit_message / regenerate_response` | 编辑 / 重新生成 |
| `fork_from_message / list_branches / switch_branch / get_branch_messages` | 分支 |

## 4. 记忆 & 评估

| 命令 | 说明 |
|------|------|
| `run_dreaming(agentId, phase)` | Light / REM 记忆整理 |
| `compile_memory_wiki(agentId, days)` | 编译 WIKI.md |
| `evaluate_character(agentId)` | 人格一致性评估 |
| `run_memory_hygiene(...)` | 记忆整理 |
| `export_memory_snapshot(agentId)` | 记忆快照 |
| `extract_memories_from_history(agentId, sessionId)` | 从对话抽取记忆 |
| `run_learner(agentId)` | 观察者学习 |

## 5. 工具 / Soul / MCP

| 命令 | 说明 |
|------|------|
| `read_soul_file / write_soul_file / list_soul_files` | SOUL.md 等 |
| `read_standing_orders / write_standing_orders` | STANDING_ORDERS.md |
| `get_agent_tools / set_agent_tool_profile / set_agent_tool_override` | 工具权限 |
| `list_mcp_servers / add_mcp_server / remove_mcp_server` | MCP 服务器 |
| `toggle_mcp_server / test_mcp_connection / import_claude_mcp_config` | MCP 管理 |

## 6. Skills / 技能

| 命令 | 说明 |
|------|------|
| `install_skill / remove_skill / list_skills / toggle_skill` | 本地技能 |
| `list_marketplace_skills / download_skill_from_hub / search_skill_hub` | 技能市场 |
| `publish_skill_to_hub / install_skill_to_agent / uninstall_skill_from_agent` | 分享 |
| `clawhub_featured / clawhub_categories / clawhub_install` | ClawHub |

## 7. Plugins / 插件

| 命令 | 说明 |
|------|------|
| `list_plugins / list_plugin_capabilities / list_system_plugins` | 列出 |
| `toggle_system_plugin / save_plugin_config / get_plugin_config` | 配置 |
| `get_agent_plugin_states / set_agent_plugin` | 绑定 |
| `import_external_plugin / install_plugin_from_url` | 导入 |

## 8. Scheduler / 定时任务

| 命令 | 说明 |
|------|------|
| `create_cron_job / update_cron_job / delete_cron_job` | CRUD |
| `list_cron_jobs / get_cron_job` | 列出 |
| `trigger_cron_job / pause_cron_job / resume_cron_job` | 控制 |
| `list_cron_runs / get_scheduler_status` | 运行历史 |

## 9. Channels / 消息渠道

| 命令 | 说明 |
|------|------|
| `create_agent_channel / list_agent_channels / delete_agent_channel / toggle_agent_channel` | 渠道绑定 |
| `weixin_get_qrcode / weixin_poll_status / weixin_save_token` | 微信 |
| `verify_telegram_token / discord_connect / slack_connect` | 其他渠道 |
| `send_poll` | 推送 |

## 10. Profile / OAuth

| 命令 | 说明 |
|------|------|
| `get_user_profile / save_user_profile / save_user_avatar / get_user_avatar` | 用户资料 |
| `start_oauth_flow / exchange_oauth_code / refresh_oauth_token` | OAuth |

## 11. 多 Agent / 协作

| 命令 | 说明 |
|------|------|
| `list_subagents / cancel_subagent / list_subagent_runs` | 子代理 |
| `approve_tool_call / deny_tool_call` | 审批 |
| `send_agent_message / get_agent_mailbox` | A2A |
| `get_agent_relations / create_agent_relation / delete_agent_relation` | 关系图 |

## 12. 语音 / 多媒体

| 命令 | 说明 |
|------|------|
| `transcribe_audio / transcribe_audio_file` | 语音转文字 |
| `start_voice_recording / stop_voice_recording` | 录音 |
| `save_chat_image / send_notification` | 其他 |

## 13. 系统 / 诊断

| 命令 | 说明 |
|------|------|
| `run_doctor / doctor_auto_fix / detect_browsers / open_in_browser` | 诊断 |
| `check_runtime / setup_runtime / get_python_sandbox_status` | 运行时 |
| `health_check` | 健康检查 |
| `get_token_stats / get_token_daily_stats / get_cache_stats` | 统计 |
| `get_setting / set_setting / get_settings_by_prefix` | 设置 |
| `backup_database / restore_database` | DB 备份 |
| `get_admin_port` | Admin HTTP 端口 |
| `export_app_data / import_app_data` | 整包迁移 |
| `cloud_api_proxy(...)` | 云网关代理 |
| `read_file_base64(path)` | 前端读本地文件（Tauri FS scope 绕过） |
| `estimate_token_cost(...)` | 估算费用 |
| `list_hooks / sop_list / sop_trigger / sop_runs` | 钩子 / SOP |
| `get_audit_log` | 审计日志 |

## 14. Plaza / 广场

| 命令 | 说明 |
|------|------|
| `plaza_create_post / plaza_list_posts` | 发帖 |
| `plaza_add_comment / plaza_get_comments / plaza_like_post` | 互动 |

## 15. 自治 / 审计

| 命令 | 说明 |
|------|------|
| `get_autonomy_config / update_autonomy_config` | 自治等级 |
| `submit_message_feedback / get_context_usage` | 反馈 / 用量 |
| `get_regeneration_info` | 重生成提示信息 |

---

## 使用示例

```typescript
import { invoke } from '@tauri-apps/api/tauri'

// 查询 admin 端口
const port = await invoke<number | null>('get_admin_port')

// 触发 Dreaming
const result = await invoke<{ phase: string, path: string, summary: string }>(
  'run_dreaming',
  { agentId: 'abc', phase: 'light' }
)

// 跨会话搜索
const hits = await invoke<any[]>('search_all_messages', { query: 'Rust async' })
```

## 错误处理

所有命令返回 `Result<T, String>`。前端应统一 try/catch：

```ts
try {
  const data = await invoke('some_command', args)
} catch (err) {
  toast.error(friendlyError(err))  // helpers/useToast.ts
}
```

## 关于脱敏与安全

- `get_providers` 会把 `apiKey` 字段整个移除，前端拿不到明文
- `approval` 请求走 `tool-approval-request` 事件，arguments 经 `redact_secrets` 处理
- `channels::find_provider` / `find_openai_provider` / `find_gemini_provider` 会自动解密 `XZ1:` 前缀

---

> 更新本文档：执行 `./scripts/gen-api-doc.sh`（待实现 — 目前手动维护）。
