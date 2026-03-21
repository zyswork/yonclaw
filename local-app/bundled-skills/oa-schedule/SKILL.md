---
name: oa-schedule
description: 用友 YonBIP 日程管理。查询日程、创建日程、邀请参与人、删除日程。当用户提到日程、安排、行程、日历、会议安排时使用此技能。
trigger_keywords:
  - 日程
  - 安排
  - 行程
  - 日历
  - schedule
  - 今天有什么
  - 这周有什么
  - 明天有什么
tools:
  - name: schedule_list
    description: 查询日程列表（默认本周，可指定日期范围）
    parameters:
      type: object
      properties:
        date:
          type: string
          description: 指定日期，如 2026-03-20
        from:
          type: string
          description: 开始日期
        to:
          type: string
          description: 结束日期
      required: []
    safety_level: guarded
    executor:
      type: command
      command: bash
      args_template: [oa-schedule.sh, list]
  - name: schedule_create
    description: 创建日程
    parameters:
      type: object
      properties:
        title:
          type: string
          description: 日程标题
        start:
          type: string
          description: "开始时间，如 2026-03-20 14:00"
        end:
          type: string
          description: "结束时间，如 2026-03-20 15:00"
        date:
          type: string
          description: 全天日程的日期
        allday:
          type: boolean
          description: 是否全天日程
        part:
          type: string
          description: 参与人 userId，逗号分隔
      required: [title]
    safety_level: approval
    executor:
      type: command
      command: bash
      args_template: [oa-schedule.sh, create]
  - name: schedule_delete
    description: 删除日程
    parameters:
      type: object
      properties:
        sid:
          type: string
          description: 日程 ID
      required: [sid]
    safety_level: approval
    executor:
      type: command
      command: bash
      args_template: [oa-schedule.sh, delete, --sid, "{sid}"]
permissions:
  read_paths: ["~/.yonclaw"]
  write_paths: []
  exec_commands: [bash, curl]
  network: true
---

# OA 日程管理技能

管理用友 YonBIP 的日程系统。支持查询、创建、邀请参与人、删除日程。

## 使用方式

```bash
# 查本周日程
bash oa-schedule.sh list

# 查指定日期日程
bash oa-schedule.sh list --date 2026-02-17

# 查指定范围日程
bash oa-schedule.sh list --from 2026-02-01 --to 2026-02-28

# 创建日程
bash oa-schedule.sh create --title "开会" --start "2026-02-17 14:00" --end "2026-02-17 15:00"

# 创建全天日程
bash oa-schedule.sh create --title "出差" --date 2026-02-18 --allday

# 创建日程并邀请参与人
bash oa-schedule.sh create --title "开会" --start "2026-02-17 14:00" --end "2026-02-17 15:00" --part "userId1,userId2"

# 邀请参与人到已有日程
bash oa-schedule.sh invite --sid <schedule_id> --users "userId1,userId2"

# 邀请抄送人（role=2）
bash oa-schedule.sh invite --sid <schedule_id> --users "userId1" --role 2

# 搜索用户（按姓名关键词，返回 ID、部门、头像）
bash oa-schedule.sh search-user "武文杰"
bash oa-schedule.sh search-user "张" 10

# 删除日程
bash oa-schedule.sh delete <schedule_id>

# 今日日程摘要（默认今天）
bash oa-schedule.sh summary

# 指定日期摘要
bash oa-schedule.sh summary 2026-03-01

# 更新日程（改标题/时间/地点等，内部用 delete+create 实现）
bash oa-schedule.sh update --sid <schedule_id> --title "新标题"
bash oa-schedule.sh update --sid <schedule_id> --start "2026-03-01 10:00" --end "2026-03-01 11:00"
bash oa-schedule.sh update --sid <schedule_id> --address "线上" --important true

```

### 参与人角色
- `--role 0` = 组织者
- `--role 1` = 参与者（默认）
- `--role 2` = 抄送人

### 参与人处理（重要！）

- 如果用户指定了参与人，创建时加 `--part <userId1,userId2>`，脚本会自动在创建后调 invite 接口添加参与人
- 如果用户没提参与人，不加 `--part`，正常创建即可
- OA 创建接口的 `invitedYhtUserIds` 字段不生效，所以脚本内部会自动走 create → invite 两步，你不需要手动调 invite
- 简单说：**有人就传 --part，没人就不传，脚本自己处理**

### 搜索参与人流程

当用户说"加XXX"但你不知道 ID 时，用 pick-user.sh 一步搞定：

```bash
bash skills/oa-common/pick-user.sh "张" "<chat_id>" 5
```

返回 JSON，根据 action 字段处理：

- **action: "single"** → 只有1人，直接用返回的 yhtUserId，回复确认
- **action: "pick_sent"** → 多人，**脚本已自动发送 Telegram 按钮**，你只需回复 NO_REPLY 等用户点按钮
- **action: "not_found"** → 零匹配，回复提示换关键词

⚠️ 多人场景你不需要自己调 message 工具，脚本已经通过 openclaw CLI 直接发了按钮！

⚠️⚠️ **pick_sent 后严禁降级为纯文本列表！** 不管你检测到的渠道是 telegram/webchat/其他，按钮已经通过脚本直接发到用户的 Telegram 了。你只需回复 NO_REPLY，不要再用文字列出候选人！

⚠️ **chat_id 从消息上下文的 sender_id 获取**

3. **多个匹配**（>=2个结果）-> **必须让用户选择**，用 Telegram inline buttons 展示：

#### 重名消歧 - Inline Buttons（必须用 message 工具发按钮！）

⚠️ **禁止用纯文本列出候选人！必须用 message 工具发 inline buttons！**

搜到多人时，调用 message 工具：

```
message(
  action: "send",
  message: "找到 3 位"张"，请选择：",
  buttons: [
    [{"text": "张波 | 办公应用开发部", "callback_data": "user_sel:9e6a0e30-75f6-49a3-93be-f7643c5fd78c"}],
    [{"text": "张新晨 | 办公应用开发部", "callback_data": "user_sel:055f1543-d0a6-4cbf-b15f-540d1ce90ef3"}],
    [{"text": "张平 | 港-高端销售部", "callback_data": "user_sel:xxx-xxx"}],
    [{"text": "❌ 取消", "callback_data": "user_sel:cancel"}]
  ]
)
```

**按钮格式规则**：
- 每人一行：`[{"text": "姓名 | 部门", "callback_data": "user_sel:<yhtUserId>"}]`
- 最后一行：`[{"text": "❌ 取消", "callback_data": "user_sel:cancel"}]`
- 发完按钮后回复 NO_REPLY（避免重复消息）

**回调处理**：
- 收到 `user_sel:<id>` 回调 → 用该 ID 继续操作
- 收到 `user_sel:cancel` → 回复"已取消"

**降级方案**（如果 message 工具发 buttons 失败或模型不支持嵌套参数）：
用编号选择，回复格式如下（加粗编号方便用户点击）：

    找到 5 位"张"，请回复编号选择：
    
    **1** 张波 - 办公应用开发部
    **2** 张新晨 - 办公应用开发部
    **3** 张平 - 港-高端销售部
    **4** 张涛 - 台-高端销售部
    **5** 张瑞行
    
    回复 0 取消

收到数字后匹配对应用户，继续执行。

多人场景：每次选定一人后问"还要加其他人吗？"，全部确认后再执行

4. **零匹配** -> 回复"未找到'XXX'，请确认姓名或换个关键词"
5. 拿到确认的 ID 后再传 `--part`

> Daily 环境搜人：bip-daily.yonyoucloud.com/yonbip-ec-contacts/contacts/candidate/web/staff/getStaffList

### 注意事项

1. 认证、搜人、HTTP 工具由 `oa-common` 公共层提供，cookie 统一在 `oa-common/cookie.txt`
2. 日程服务 base URL 是 c2.yonyoucloud.com（不是 c1）
3. 时间戳用秒级（不是毫秒）
4. 添加参与人使用 `/api/v2/share` 接口（脚本在 create 带 --part 时自动调用）
5. targetUsers 传 yhtUserId 列表，需要先通过 search-user 获取 ID
