# OA 公共层

用友 YonBIP 的公共能力，被 oa-task 和 oa-schedule 等业务脚本共享。

## 提供的能力

### 认证
- 统一读取 `cookie.txt`，提取 XSRF-TOKEN、tenantId
- 所有业务脚本共用同一份 cookie

### 通用 HTTP
- `oa_post(url, data)` / `oa_get(url)` / `oa_delete(url)` / `oa_put(url, data)`
- 自动带 cookie、XSRF-TOKEN、origin/referer 头

### 搜索用户
- `oa_search_user <keyword> [size]`
- 返回：姓名、ID、部门、公司、头像 URL
- 同名时列出所有匹配，由调用方选择

### 日期工具
- `oa_date_to_ts <datetime_string>` — 多格式日期转秒级时间戳

## 使用方式

业务脚本通过 `source` 引入：
```bash
source "$SCRIPT_DIR/../oa-common/oa-common.sh"
```

## 文件结构
```
skills/
  oa-common/
    oa-common.sh    # 公共层脚本
    cookie.txt      # 统一认证 cookie
  oa-task/
    oa-task.sh      # source oa-common
  oa-schedule/
    oa-schedule.sh  # source oa-common
```

## Cookie 更新
Cookie 过期后只需更新 `oa-common/cookie.txt` 一处，所有业务脚本自动生效。
