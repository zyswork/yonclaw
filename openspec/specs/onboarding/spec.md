# 首次使用引导

## 能力
系统 SHALL 在首次启动时引导用户完成：Provider 选择 → 填入 API Key → 创建首个 Agent。

## 需求

### Requirement: 检测首启
系统 SHALL 检查 DB 中 `agents` 表是否为空，空则导向 `SetupPage`，否则直接进入主界面。

### Requirement: 引导流程
SetupPage SHALL 至少包含：
- **Step 1** Provider 选择（OpenAI / Anthropic / Zhipu / MiMo / Ollama / LM Studio）
- **Step 2** 填 API Key（或 localhost 本地服务）
- **Step 3** 创建第一个 Agent（系统/用户提示 + 模板）

### Requirement: 可跳过
系统 SHALL 允许"跳过引导"直接进入，后续可在设置里配置。

## 命令面板 (Cmd+K)

### Requirement: 全局快捷键
系统 SHALL 响应 Cmd+K / Ctrl+K 打开命令面板覆盖层。

### Requirement: 统一搜索
面板 SHALL 至少包含：
- 所有主页面（Dashboard / Agents / Skills / Cron / Memory / Plugins / Channels / Settings / Doctor）
- 所有已创建 Agent（点击跳转 AgentDetail）

### Requirement: 键盘操作
- **↑↓** 切换选中
- **Enter** 执行
- **Esc** 关闭
