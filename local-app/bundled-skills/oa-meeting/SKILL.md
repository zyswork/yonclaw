---
name: oa-meeting
description: 用友 YonBIP 会议室预定管理。查询空闲会议室、预定会议、修改会议、取消/删除会议、查看我的会议。支持按位置/容量/时间筛选会议室，搜索参会人。当用户提到会议室、订会议室、预定会议、开会时使用此技能。
trigger_keywords:
  - 会议室
  - 订会议室
  - 预定会议
  - 开会
  - 会议
  - meeting
  - 空闲会议室
permissions:
  read_paths: ["~/.yonclaw"]
  write_paths: []
  exec_commands: [bash, curl]
  network: true
---

# OA 会议室预定技能

## 适用场景 (Trigger)
- 用户要求预定/订会议室
- 用户要求查看空闲会议室
- 用户要求修改/取消/删除会议
- 用户要求查看"我的会议"/"今天的会议"
- 用户要求给会议加人/换人
- 用户提到"约个会"/"开个会"/"找个会议室"

## 禁用边界 (Red Lines)
- 禁止在群聊中暴露 cookie/token
- 预定会议前必须确认：会议室 + 时间 + 参会人 + 主题
- 取消/删除会议需经 SpiderMan 确认
- Cookie 过期时提醒用户刷新，不要反复重试

## 环境配置

| 项目 | 值 |
|------|-----|
| **API 基址** | `https://c1.yonyoucloud.com/yonbip-ec-meeting` |
| **联系人 API** | `https://c2.yonyoucloud.com/yonbip-ec-contacts` |
| **Cookie** | `skills/oa-meeting/cookie.txt` |
| **回退 Cookie** | `~/.openclaw/workspace-eva-bot/skills/oa-common/cookie.txt` |
| **认证方式** | Cookie + XSRF-TOKEN 请求头 |
| **Cookie 过期标志** | HTTP 302 / 401 / 空响应 |
| **Tenant** | `qyic8c7o` |
| **用户 ID** | `c6240951-4fe0-4981-87c9-476125365627` |

## 脚本位置
```
skills/oa-meeting/oa-meeting.sh
```

## 命令清单

### 1. 查看我的会议
```bash
bash oa-meeting.sh my today    # 今天的会议
bash oa-meeting.sh my week     # 本周的会议
bash oa-meeting.sh my all      # 所有会议（最近20条）
```

### 2. 查询空闲会议室
```bash
bash oa-meeting.sh rooms 2026-03-02                          # 某天所有会议室
bash oa-meeting.sh rooms 2026-03-02 西区                      # 按位置过滤
bash oa-meeting.sh rooms 2026-03-02 "" 10:00 11:00           # 按时间过滤空闲
bash oa-meeting.sh rooms 2026-03-02 "1号楼" 14:00 15:00      # 位置+时间
```

### 3. 预定会议
```bash
bash oa-meeting.sh book '{
  "subject": "讨论AI落地",
  "meetingRoomId": "1944783165066313728",
  "meetingRoomName": "1C103",
  "date": "2026-03-02",
  "startTime": "15:00",
  "endTime": "16:00",
  "personList": [
    {"commonUserId": "xxx", "userName": "武文杰", "isParticipant": 1, "isRole": 0},
    {"commonUserId": "yyy", "userName": "刘英文", "isParticipant": 1, "isRole": 0}
  ]
}'
```

**personList 字段映射**:
| 来源字段 (search-user) | 目标字段 (personList) |
|------------------------|----------------------|
| `yhtUserId` | `commonUserId` |
| `userName` | `userName` |
| 固定值 1 | `isParticipant` |
| 固定值 0 | `isRole` |

### 4. 修改会议
```bash
bash oa-meeting.sh edit <meetingId> '{"subject":"新主题"}'
bash oa-meeting.sh edit <meetingId> '{"startTime":"14:00","endTime":"15:30"}'
bash oa-meeting.sh edit <meetingId> '{"subject":"新主题","personList":[...]}'
```

**⚠️ edit 接口的时间格式**:
- 使用 `meetingDateTime` / `startDateTime` / `endDateTime` (Long 毫秒时间戳)
- **不要**传 `meetingTime` / `startTime` / `endTime` (Date 类型，会报 "Cannot format given Object as a Date")
- 脚本已内部处理转换，调用者只需传 `"startTime":"HH:MM"` 格式

### 5. 取消会议
```bash
bash oa-meeting.sh cancel <meetingId>              # 无原因
bash oa-meeting.sh cancel <meetingId> "时间冲突"    # 带原因
```

### 6. 删除会议
```bash
bash oa-meeting.sh delete <meetingId>
```

### 7. 查看会议详情
```bash
bash oa-meeting.sh detail <meetingId>
```

### 8. 搜索参会人
```bash
bash oa-meeting.sh search-user "武文杰"
bash oa-meeting.sh search-user "刘英"
```

## 典型工作流

### 预定会议的完整流程
1. **确认参会人**: `search-user` 搜索姓名 → 拿到 `yhtUserId`
2. **查空闲会议室**: `rooms <date> [location] [start] [end]` → 选择合适的
3. **确认信息**: 向 SpiderMan 确认 主题 + 会议室 + 时间 + 参会人
4. **提交预定**: `book <json>`

### API 端点汇总

| 操作 | 方法 | 路径 |
|------|------|------|
| 我的会议 | GET | `/meeting/myAttendMeeting?pageNum=1&pageSize=20` |
| 查会议室 | GET | `/meeting/checkMeetingInformation?date={timestamp}&pageNum=1&pageSize=200` |
| 创建会议 | POST | `/meeting/predestinate` |
| 修改会议 | POST | `/meeting/edit/{meetingId}` |
| 取消会议 | POST | `/meeting/cancelMeeting/{meetingId}` |
| 删除会议 | DELETE | `/meeting/deleteMeeting/{meetingId}` |
| 搜索用户 | POST | (via oa-common `oa_search_user`) |

## 已知坑点
1. **predestinate vs edit 时间格式不同**: 创建用 `startTime`(HMS offset) + `startDateTime`(full)；编辑只用 `meetingDateTime`/`startDateTime`/`endDateTime`(Long)
2. **HMS 时间戳**: predestinate 的 `startTime` 是 1970-01-01 基准的毫秒偏移（UTC），如 15:00 CST = 07:00 UTC = 25200000
3. **c1 需要 JSESSIONID**: 没有这个 cookie 请求会返回空或 302
4. **搜索用户跨域**: contacts API 在 c2，origin/referer 设为 c2（oa-common 已处理）
5. **会议室 ID 是字符串**: 不是数字，如 `"1944783165066313728"`
