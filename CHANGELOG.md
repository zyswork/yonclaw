# Changelog

所有值得注意的变更都记录在此。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本遵循 [SemVer](https://semver.org/lang/zh-CN/)。

## [Unreleased]

### Added

- **拖拽文档自动抽取**：拖 PDF / DOCX / XLSX / MD / JSON / CSV / 等（14 种后缀）到聊天框，前端立即调 `parse_document` Tauri 命令 → 后端走 `doc_parse` 工具同一份解析逻辑 → `[文档: xxx]\n<正文>\n[文档结束]` 块插入输入框。max 30000 字符/文件。不支持的后缀降级为仅插入路径（原行为保留）。效果：免去"贴路径 → AI 调 doc_parse → 读结果 → 再回答"两轮对话。
- **Link Understanding（`agent/link_understanding.rs`）**：用户消息里的 URL 在送入 LLM 前自动抽取标题/描述/正文前 500 字，作为 `[链接摘要]` 上下文块前置。免去 LLM 显式调用 `web_fetch` 一来一回。前 3 个 URL 并行抓取，各 5s 超时，HTML 解析剥 script/style/tag，支持 `<article>`/`<main>` 优先正文。复用 `web_fetch` 的 SSRF/内网/DNS rebind 防护。失败静默跳过不阻塞主流程。7 个单元测试全绿。
- **Updater GitHub raw fallback**：`tauri.conf.json` 配置双 endpoint，主站 `zys-openclaw.com` 外多一个 `raw.githubusercontent.com/zyswork/xianzhu-claw/main/updater/latest.json`。主域名挂/备案异常时自动降级到 GitHub CDN，用户永远能收到更新。
- **Updater 发布脚本 + 文档**：`updater/README.md` 描述 jq 一键更新 version+signature 流程。

### Changed

- **SetupPage 去硬编码 gpt-4o**：首次引导不再 step 0 就创建 `model: 'gpt-4o'` agent。改为：保存 provider 时调 `test_provider_connection` 拿真实可用模型列表，过滤掉 embedding/tts/whisper/moderation 后挑第一个 chat 模型补建 agent。Deepseek / Gemini / Qwen 用户也能走通引导。
- **`test_provider_connection` 返回 `model_ids`**：原本只返回 count，现在额外给前 20 个模型 ID（供 SetupPage / Agent 创建时选用）。

### Fixed

- **ChatTab `messages` 偶发 null crash**：`get_session_messages` 返回 null 时直接塞给 state，渲染时 `messages.length` 爆 `Cannot read properties of null`。改 `setMessages(msgs || [])`。由 smoke test 触发发现。

### Removed

- **死码 `spawn_subagent`**：65 行方法 + 8 行 `SpawnConfig` struct，全程零外部调用方。多 agent 实际走 `delegate.rs`。删除避免后续看到 `"后台执行尚未实现"` 注释误判为 bug。

### Tests

- **ChatTab smoke tests（`ChatTab.test.tsx`，7 个用例）**：渲染不崩 / agentId 切换重载 session / session 列表渲染 / PDF 拖拽触发 parse_document / png 不触发 / zip 等非文档仅插入路径。覆盖 3500 行核心组件的关键入口。顺带发现并修了 null-crash。
- **jsdom polyfill**：`Element.prototype.scrollIntoView` 加到全局 setup，让使用了滚动定位的组件能测。
- **总测试数** 39 → 46（+7），9 个 test file 全绿。

## [1.1.0] — 2026-04-20

OpenClaw（50+ commits）+ Hermes Agent 同步 + Santorini 主题重塑 + 6 轮安全审计修复。

### Added — 新能力

- **Santorini 主题系统**：Vone Lin 配色方案（Calm Bay `#C7DDEA` / Observer Lavender `#D8CEE4` / Truth Warm Orange `#FFB347`），浅色默认 + 深色 opt-in。浅色纯净暖白 + 极淡 mesh 氛围；深色低饱和夜蓝灰 + 壁纸透出。透明窗口 + 动画光晕（`prefers-reduced-motion` 支持）。
- **Dreaming 定时触发正式落地**：`ActionPayload::Dreaming { phase }` scheduler 变体 —— Light Sleep 每日 03:00、REM Sleep 每周日 03:30 自动跑 `run_dream_phase`，输出落 `memory/dreaming/{phase}/YYYY-MM-DD.md`。原来的 prompt 驱动方式改为结构化调用。
- **插件 text_transforms 接入 orchestrator**：PreLlm regex 变换在 `send_message_stream` 入口生效，术语替换 / 模板展开 / 敏感词过滤真正可用。
- **Dreaming / REM 记忆整理**：参照 OpenClaw #63273/#63297，每日凌晨 3:00 自动 Light Sleep 浅睡整理，周日凌晨 4:00 REM 深度复盘。斜杠命令 `/dream [light|rem]` 手动触发。
- **Memory Wiki**：聚合最近 N 天的 REM 记忆为 `WIKI.md`，斜杠命令 `/wiki [days]` 编译。
- **Character Eval**：人格一致性评估（Hermes），`/character` 斜杠命令 + 每周日自动跑。
- **Canvas 侧边面板**：代码块 "Canvas" 按钮打开侧边编辑器，支持行号、Tab 缩进、复制下载、"插入到输入框"。
- **连续语音对话模式**：紫色循环按钮进入 "录音→ASR→发送→回复→TTS→录音" 闭环。
- **Model Auth 状态卡**：Dashboard 显示各 Provider 的 key 健康度 / OAuth 过期 / 未配置数量。
- **文件 Diff 可视化**：`<!--diff:{...}-->` 标签自动渲染红/绿差异行。
- **Assistant Embed 指令**：`<!--embed:{type:url|iframe|quote, src, title}-->` 标签自动渲染为内联组件。
- **插件文本 transforms**（`plugin_system/text_transforms.rs`）：regex 输入/输出过滤，预留 hook 点。
- **可插拔 Context Engine**（`agent/context_engine.rs`）：trait + Registry 架构，默认 `LegacyEngine`。
- **本地 Admin HTTP 端点**：127.0.0.1 随机端口只读状态查询（`/status` + `/agents`），跨域拒绝。
- **GhRead 工具**：只读 GitHub API（repo / file / issues / pulls / commits / releases）。
- **Gemini TTS**：Google Gemini 2.5 Flash TTS 作为新 TTS mode。
- **Seedance 2 / HeyGen 视频模型**：通过 fal.ai 调用。
- **LM Studio provider 预设**：本地端口 1234 的 OpenAI 兼容服务。
- **CLI `infer` 子命令**：一次性推理，支持 `--model` `--system` `--json`。
- **离线检测横幅**：断网 / 恢复时顶部提示。
- **Dreaming + Character eval + Memory Wiki 自动化种子任务**：首次启动注入。

### Changed — 体验改进

- **prettyModel**：Dashboard / AgentList / AgentDetail / ChatTab 多 Agent 面板用 provider display name 替换 `custom-<timestamp>` 自动 ID。
- **Memory XML fencing**：召回记忆包 `<memory-context>` + 系统注释，防 LLM 误作当前任务（Hermes）。
- **memory-wiki belief-layer digests**：合并多天 REM 为单一 wiki 供检索。
- **system prompt 威胁扫描**（`agent/threat_scan.rs`）：SOUL/USER/IDENTITY/MEMORY.md 注入前 regex + 剥离不可见 Unicode。
- **MEMORY.md 2200 / USER.md 1375 字符上限**，分域独立去重（Hermes）。USER.md 头部保留（手编文件语义），MEMORY.md 尾部保留（append-log 语义）。
- **Fuzzy file edit**：`file_edit` 工具缩进容忍 + 歧义匹配检测（Hermes）。
- **结构化 compaction**：`Context / Resolved / Pending / Key Identifiers` 模板 + tool 输出预压缩 + `<session-summary>` 包裹。
- **429 退避改进**：优先读 `Retry-After` header 和响应体 `retryDelay`，否则 10s/30s/60s 积极退避（原为 2s/4s）。
- **Smart model routing**：13 维复杂度评分自动分流 light / standard / heavy 模型。
- **Session 消息缓存失效**：`clear_history` / `delete_session` / `edit_message` / `regenerate_response` / `fork_from_message` / `switch_branch` 全部正确 invalidate。
- **审批卡片 redact 密钥**：扩展覆盖 `sk-ant-` / `AIza` / `github_pat_` / `ghu_` / `ghr_` / `xoxp-` / `AKIA` / `ASIA` 等 9 种前缀。
- **Subagent / cli / channel 的 apiKey 解密**：`channels::find_provider`、`find_openai_provider`、`find_gemini_provider` 补齐 `XZ1:` 解密。
- **斜杠命令去重 toast**：`/dream` `/wiki` `/character` 不再双通知。

### Fixed — 重要修复

- **🔴 SQL 注入 — `scheduler::list_jobs`**：`agent_id` 通过 `format!` 直接拼 SQL，攻击者可注入。全改为 `.bind()` 参数化。
- **🔴 chat_messages 新库缺列**：`parent_id` / `branch_id` 仅在 ALTER 里加，但 ALTER 执行在 CREATE 之前 → 新用户首装后 branch/fork 功能直接报 `no such column`。修复：并入 CREATE TABLE 本体。
- **🟠 SSE UTF-8 chunk 切断**：流式中文/emoji 跨 chunk 边界时 `from_utf8_lossy` 两边都替换为 U+FFFD，字符丢失。加 `pending_bytes` 缓冲按完整 UTF-8 前缀解码。
- **🟠 delete_session 与流式生成 race**：删除时流仍在写 → 孤儿消息。先 `cancel_session()` + 80ms 等退出再删。
- **🟠 Agent 工具链取消响应慢**：串行多工具时只在 round 顶检查 cancel → 用户点停止后仍跑完剩余工具。每个工具前补 cancel check + 跳出 round 循环。
- **🟠 file_read 无大小上限**：AI 误读 GB 级日志 OOM。加 10MB 上限 + 明确错误提示。
- **🟠 plugin 同 ID 重复加载 providers 堆叠**：`web_search/image_gen/tts_providers.extend()` 不去重 → dispatcher 路由乱。入口拒绝重复 ID。
- **🟡 dreaming 错误分类**：DB 查询错被 `.ok().flatten()` 吞噬 → "agent 不存在"与"DB 挂了"无从区分。改显式 match。"最近无对话" / "无新观察"改为 Success 不污染失败率。
- **🟡 CronPage Tauri 事件监听 race**：`listen()` promise 未 resolve 前组件卸载，unlisten 永久泄漏。加 `cancelled` 标志 + 延迟 unlisten。
- **🟡 AgentDetailPage 卸载后 setState**：快速切换 agent 时旧 fetch 完成对已卸载组件 setState 报错。加 `cancelled` 标志。
- **🟡 ChatTab Python sandbox 轮询泄漏**：`setInterval` + `setTimeout` 未在 cleanup 清理，卸载后仍在跑。捕获 handle 在 cleanup 清掉。
- **🟡 MCP 子进程 zombie**：`child.kill()` 仅发信号不 wait → 子进程僵尸化。加 `child.wait()` + 2s 超时兜底。
- **🟡 KEY_ROTATOR 毒锁 panic**：`.lock().unwrap()` 遇到 panic 中毒 mutex → 整个命令返回模糊 IPC 错误。改 `unwrap_or_else(\|p\| p.into_inner())` 毒锁恢复。
- **🟡 EventBroadcaster 丢事件**：broadcast channel capacity 100，长任务多订阅者易 Lagged。提升到 512。
- **🟡 migration ALTER 静默失败**：`let _ =` 吞掉一切错误。加 `alter_idempotent` 辅助函数：`duplicate column` 静默，其他错误 `log::warn!`。
- **Cron 页 i18n 泄漏 + 双 "+"**：`cron.searchPlaceholder` 键名泄漏，按钮 icon + i18n 都有 "+" 显示 "+ +"。补翻译 + 去 icon。
- **Layout `<main>` 覆盖 mesh**：主区 `backgroundColor: var(--bg-base)` 让 body 的动画光晕看不到。改 `transparent`。
- **Dashboard "缺少 API Key" 假阴性**：`get_providers` 脱敏后 `apiKey` 被 remove，前端改用 `apiKeyCount` 判断。
- **Subagent 测试 API Key 全失效**：`channels::find_provider` 读 DB 未解密 → 子代理拿到密文 `XZ1:...` 发 API → 401。
- **`custom-<timestamp>/model` 原始 ID 泄漏**：3 处前端 UI + `agent_self_config` 工具输出都替换为 provider display name。
- **OAuth URL 验证器阻止 `&`**：改用 `url::Url::parse`，只拒控制字符 + 非 http(s) scheme。
- **Admin server CORS `*` 跨域泄漏**：改为拒绝任何带 `Origin` header 的请求（Origin check 前置到 DB 查询之前）。
- **UTF-8 byte-slice panic**：`builtin.rs` 多处 `body[..N.min(len)]` 改为 `chars().take(N)`。
- **插件下载 OOM**：`install_plugin_from_url` 改为流式累积，Content-Length 预检 + 256KB 硬上限。
- **UnknownTool 分类过松**：收紧为 `starts_with("工具不存在:")` 精确前缀，避免误触发 bash `command not found`。
- **Windows cmd.exe 注入**：OAuth 打开 URL 改用 `rundll32 url.dll,FileProtocolHandler`，非 `cmd /C start`。
- **`<function>` 标签 / `commentary` 标签 / `<think>` 标签**：统一流式过滤不泄漏给用户。
- **Ollama `ollama/` 前缀剥离**：否则 Ollama API 返 404。
- **Anthropic / OpenAI max_tokens 非正整数校验**：避免上游 400。
- **compaction 预算下限 25%**：小上下文本地模型（16K Ollama）防无限 compaction 循环。
- **session_expired 错误归类**：`No conversation found` / `input item id does not belong` 提示新建会话而非认证失败。
- **OpenRouter 404 JSON 模型错误**分类。
- **Qwen3 `reasoning_details`** OpenRouter 流式解析。
- **memory_read 截断 + continuation**：默认每条 800 字符，超出标记 `truncated` + `total_chars` + `continuation` 提示。
- **call_openai / call_anthropic 非流式加 3 次重试**，5xx/429 积极退避。
- **marked 升级到 18.x** 关闭已知 ReDoS。
- **审批超时主动 `expire()`**：避免竞态下用户点击"批准"后仍被判超时。
- **审批超时文案 30s 对齐实际值**。
- **Tauri `get_audit_log` 重复注册**：删除一处。
- **子代理 / CLI 会话前缀** `[subagent]` / `[cross-agent]` / `cli-infer-` 纳入 `isSystemSession`，收进折叠区；清理按钮一键清全部。
- **`compress_tool_output` false-positive**：`error` 子串误标工具错误 → 改为行首 `Error:` / `错误:` / `exit code != 0`。
- **`<memory-context>` 转义**：记忆内容中出现的闭合标签字面量替换为 `</memory_context>`（下划线）。
- **ChatML / Llama3 / Mistral `[INST]` / 通配 `<|...|>`** 注入模式全覆盖。
- **Fuzzy replace 多级缩进损坏**：改为 delta 缩进补偿，保留 new_text 相对缩进，歧义匹配时拒绝替换。
- **last_user_request stale**：compact 从全部消息取最新 user，不从已压缩段取。

### Security

- **审批卡片自动 redact 密钥**（XOR+base64 encryption `XZ1:` 前缀 + 敏感字段名 + 密钥 prefix 模式）。
- **SSRF 防护**：web_fetch / GitHub API / fal.ai 请求都走 https 白名单。
- **Path traversal**：`install_plugin_from_url` 插件名 normalize + 拒 `../`。
- **Safe-bin 白名单**：30+ 低风险命令绕过审批，其他 bash 命令需确认。
- **fs-safe symlink**：`validate_path_safety` 递归解析 + 系统路径黑名单。

### Performance

- **Embedding 请求显式 60s timeout**，连接 10s。
- **gpt-5.4-pro 256K 上下文窗口**前向兼容。

### Infrastructure

- **admin_server 优雅关闭**：`shutdown_signal()` + `tokio::sync::Notify`。
- **自动 character eval cron**：周日 04:00。
- **Dreaming Light Sleep cron**：每日 03:00。

### Stats

- 34+ 文件修改 + 8 新文件 + UI 主题系统重构
- 262 tests PASS，clippy 零 warning
- **6 轮安全审计**：共修复 12 个真 bug（2 🔴 高危 / 6 🟠 中危 / 6 🟡 低危 / 含前端 3 处泄漏），驳回约 35 个误报

---

## 历史版本

### Pre-sync

之前的版本记录见 git 历史。
