---
name: oa-task
description: 用友 YonBIP 任务管理。查询任务、创建任务、完成任务、取消任务。当用户提到任务、待办、todo、工作项时使用此技能。
trigger_keywords:
  - 任务
  - 待办
  - todo
  - 工作项
  - task
  - 有什么任务
  - 创建任务
tools:
  - name: task_list
    description: 查询任务列表（默认进行中的任务）
    parameters:
      type: object
      properties:
        status:
          type: string
          description: "任务状态过滤：doing(进行中)/done(已完成)/cancel(已取消)"
        keyword:
          type: string
          description: 关键词搜索
      required: []
    safety_level: guarded
    executor:
      type: command
      command: bash
      args_template: [oa-task.sh, list]
  - name: task_create
    description: 创建任务（支持自然语言）
    parameters:
      type: object
      properties:
        name:
          type: string
          description: 任务名称
        importance:
          type: integer
          description: "重要程度：0(普通) 1(重要) 2(紧急)"
        text:
          type: string
          description: 自然语言描述（使用 AI 解析创建）
      required: []
    safety_level: approval
    executor:
      type: command
      command: bash
      args_template: [oa-task.sh, create]
  - name: task_complete
    description: 完成任务
    parameters:
      type: object
      properties:
        task_id:
          type: string
          description: 任务 ID
      required: [task_id]
    safety_level: approval
    executor:
      type: command
      command: bash
      args_template: [oa-task.sh, complete, "{task_id}"]
permissions:
  read_paths: ["~/.xianzhu"]
  write_paths: []
  exec_commands: [bash, curl]
  network: true
---

# OA 任务管理技能

管理用友 YonBIP 的任务系统。支持查询、创建、完成、取消、重启任务。

## 使用方式

所有接口通过 `oa-task.sh` 脚本调用，脚本处理认证和请求。

### 环境要求

Cookie 配置在 `cookie.txt` 中（临时方案，后续改为自动刷新）。

### 可用命令

```bash
# 查任务列表（默认查进行中的）
bash oa-task.sh list

# 查任务列表（带过滤）
bash oa-task.sh list --status done
bash oa-task.sh list --keyword "周报"

# 查任务详情
bash oa-task.sh detail <task_id>

# 创建任务
bash oa-task.sh create --name "任务名称" --importance 1

# AI 文本转任务（自然语言创建）
bash oa-task.sh ai-create "明天下午3点开会讨论项目进度"

# 完成任务
bash oa-task.sh done <task_id>

# 取消任务
bash oa-task.sh cancel <task_id>

# 重启任务
bash oa-task.sh restart <task_id>

# 查任务分类
bash oa-task.sh categories

# 查任务来源
bash oa-task.sh sources
```

### 参数说明

- `--status`: 任务状态过滤（doing=进行中/done=已完成/pending=待处理/cancel=已取消/all=全部）
- `--keyword`: 关键词搜索
- `--importance`: 重要程度（1=普通, 2=重要, 3=紧急）
- `--start`: 开始时间（格式：2026-02-16 14:00:00）

### 注意事项

1. 认证、搜人、HTTP 工具由 `oa-common` 公共层提供，cookie 统一在 `oa-common/cookie.txt`
2. 创建任务必填字段：name, importance, enableNotify, startTime, chargeYhtUserId
3. chargeYhtUserId 默认为当前用户，可通过 --charge 指定其他人
4. 指定负责人时如果用户说的是姓名，先用 oa-common 的 `oa_search_user` 搜索：
   - 唯一匹配 -> 直接用
   - 多个匹配 -> 用 `pick-user.sh` 搜人+发按钮：
   ```bash
   bash skills/oa-common/pick-user.sh "张" "<chat_id>" 5
   ```
   返回 JSON：
   - action="single" → 直接用 yhtUserId
   - action="pick_sent" → **脚本已自动发按钮**，回复 NO_REPLY 等回调
   - action="not_found" → 提示换关键词
   - 收到 `user_sel:<id>` 回调 → 用该 ID 继续


### 更新任务

```bash
# 更新任务名称和优先级（内部用 cancel+create 实现，会生成新 ID）
bash oa-task.sh update --id <task_id> --name "新名称" --importance 3

# 更新描述
bash oa-task.sh update --id <task_id> --desc "新描述"
```

**注意**: update 内部是 cancel 旧任务 + create 新任务，所以 task ID 会变。如果有参与人/附件，需要在新任务上重新添加。

### 用户缓存

搜索用户时，结果会自动缓存到 `oa-common/recent-users.json`（最多 30 人）。

```bash
# 查缓存
node skills/oa-common/user-cache.js lookup "张"

# 添加到缓存
node skills/oa-common/user-cache.js add '{"name":"张波","yhtUserId":"xxx","deptName":"开发部"}'

# 列出全部缓存
node skills/oa-common/user-cache.js list
```
