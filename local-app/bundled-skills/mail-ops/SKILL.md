---
name: mail-ops
version: 1.0.0
description: 邮箱操作技能：通过 IMAP/SMTP 查看、搜索、读取、发送邮件
trigger_keywords:
  - 邮件
  - 邮箱
  - 收件箱
  - 发邮件
  - mail
  - email
  - inbox
permissions:
  read_paths: ["~/.yonclaw"]
  write_paths: []
  exec_commands: [python3, security]
  network: true
tools:
  - name: mail_list
    description: 列出最新邮件（默认收件箱最近 10 封）
    parameters:
      type: object
      properties:
        limit:
          type: integer
          description: 返回邮件数量，默认 10
        folder:
          type: string
          description: 邮箱文件夹，默认 INBOX
        days:
          type: integer
          description: 查看最近 N 天的邮件
      required: []
    safety_level: guarded
    executor:
      type: script
      path: mail_ops.py
      interpreter: python3
  - name: mail_search
    description: 搜索邮件（按关键词搜索主题和发件人）
    parameters:
      type: object
      properties:
        keyword:
          type: string
          description: 搜索关键词
        limit:
          type: integer
          description: 最多返回数量，默认 10
        folder:
          type: string
          description: 邮箱文件夹，默认 INBOX
      required: [keyword]
    safety_level: guarded
    executor:
      type: script
      path: mail_ops.py
      interpreter: python3
  - name: mail_read
    description: 读取指定邮件的完整正文
    parameters:
      type: object
      properties:
        uid:
          type: string
          description: 邮件 UID
        folder:
          type: string
          description: 邮箱文件夹，默认 INBOX
      required: [uid]
    safety_level: guarded
    executor:
      type: script
      path: mail_ops.py
      interpreter: python3
  - name: mail_send
    description: 发送邮件
    parameters:
      type: object
      properties:
        to:
          type: string
          description: 收件人，多个用逗号分隔
        cc:
          type: string
          description: 抄送，多个用逗号分隔
        subject:
          type: string
          description: 邮件主题
        body:
          type: string
          description: 邮件正文
      required: [to, subject, body]
    safety_level: approval
    executor:
      type: script
      path: mail_ops.py
      interpreter: python3
  - name: mail_mark_read
    description: 标记邮件为已读
    parameters:
      type: object
      properties:
        uid:
          type: string
          description: 邮件 UID
        folder:
          type: string
          description: 邮箱文件夹，默认 INBOX
      required: [uid]
    safety_level: guarded
    executor:
      type: script
      path: mail_ops.py
      interpreter: python3
  - name: mail_delete
    description: 删除邮件
    parameters:
      type: object
      properties:
        uid:
          type: string
          description: 邮件 UID
        folder:
          type: string
          description: 邮箱文件夹，默认 INBOX
      required: [uid]
    safety_level: approval
    executor:
      type: script
      path: mail_ops.py
      interpreter: python3
requires:
  bins: [python3]
  env: []
---

# 邮箱操作技能

通过 IMAP/SMTP 操作邮箱，支持查看邮件、搜索邮件、读取正文、发送邮件，并提供更安全、更可排错的配置方式。

## 安全说明

**不要把邮箱密码写入仓库或聊天记录。**

本技能支持以下配置来源，优先级从高到低大致为：
- 环境变量
- 技能目录下 `.env`
- 技能目录下 `config.local.json`
- 代码默认值

推荐优先使用：
- 环境变量
- `MAIL_IMAP_PASSWORD_CMD` / `MAIL_SMTP_PASSWORD_CMD`
- 本机私密配置文件 `config.local.json`

## 支持的配置项

### 基础配置

- `MAIL_IMAP_HOST`：IMAP 服务器地址，默认 `mail.yonyou.com`
- `MAIL_IMAP_PORT`：IMAP 端口，默认 `993`
- `MAIL_IMAP_SSL`：IMAP 是否使用 SSL，默认 `true`
- `MAIL_SMTP_HOST`：SMTP 服务器地址，默认 `mail.yonyou.com`
- `MAIL_SMTP_PORT`：SMTP 端口，默认 `465`
- `MAIL_SMTP_SSL`：SMTP 是否使用 SSL，默认 `true`
- `MAIL_SMTP_STARTTLS`：SMTP 是否启用 STARTTLS，默认 `false`
- `MAIL_USER`：邮箱账号，例如 `zhangyshp@yonyou.com`
- `MAIL_INBOX`：默认邮箱文件夹名，默认 `INBOX`
- `MAIL_DEBUG`：是否开启调试日志，默认 `false`

### 密码配置

支持以下几种方式：

- `MAIL_IMAP_PASS`：单独指定 IMAP 密码
- `MAIL_SMTP_PASS`：单独指定 SMTP 密码
- `MAIL_PASS`：IMAP/SMTP 共用密码
- `MAIL_IMAP_PASSWORD_CMD`：通过命令输出 IMAP 密码
- `MAIL_SMTP_PASSWORD_CMD`：通过命令输出 SMTP 密码
- `MAIL_PASSWORD_CMD`：IMAP/SMTP 共用密码命令

优先级大致如下：

- IMAP：`MAIL_IMAP_PASS` > `MAIL_IMAP_PASSWORD_CMD` > `MAIL_PASS` > `MAIL_PASSWORD_CMD`
- SMTP：`MAIL_SMTP_PASS` > `MAIL_SMTP_PASSWORD_CMD` > `MAIL_PASS` > `MAIL_PASSWORD_CMD`

这样可以完全对齐 Himalaya 的方式：
- IMAP 从一个命令读取
- SMTP 从另一个命令读取

## 按 Himalaya 风格配置示例

```bash
MAIL_IMAP_HOST=mail.yonyou.com
MAIL_IMAP_PORT=993
MAIL_IMAP_SSL=true
MAIL_SMTP_HOST=mail.yonyou.com
MAIL_SMTP_PORT=465
MAIL_SMTP_SSL=true
MAIL_SMTP_STARTTLS=false
MAIL_USER=zhangyshp@yonyou.com
MAIL_IMAP_PASSWORD_CMD=security find-generic-password -a 'zhangyshp@yonyou.com' -s 'himalaya-imap' -w
MAIL_SMTP_PASSWORD_CMD=security find-generic-password -a 'zhangyshp@yonyou.com' -s 'himalaya-smtp' -w
MAIL_INBOX=INBOX
MAIL_DEBUG=false
```

## `.env` 示例

```bash
MAIL_IMAP_HOST=mail.yonyou.com
MAIL_IMAP_PORT=993
MAIL_IMAP_SSL=true
MAIL_SMTP_HOST=mail.yonyou.com
MAIL_SMTP_PORT=465
MAIL_SMTP_SSL=true
MAIL_SMTP_STARTTLS=false
MAIL_USER=zhangyshp@yonyou.com
MAIL_PASS=
MAIL_IMAP_PASS=
MAIL_SMTP_PASS=
MAIL_PASSWORD_CMD=
MAIL_IMAP_PASSWORD_CMD=security find-generic-password -a 'zhangyshp@yonyou.com' -s 'himalaya-imap' -w
MAIL_SMTP_PASSWORD_CMD=security find-generic-password -a 'zhangyshp@yonyou.com' -s 'himalaya-smtp' -w
MAIL_INBOX=INBOX
MAIL_DEBUG=false
```

## `config.local.json` 示例

```json
{
  "MAIL_IMAP_HOST": "mail.yonyou.com",
  "MAIL_IMAP_PORT": 993,
  "MAIL_IMAP_SSL": true,
  "MAIL_SMTP_HOST": "mail.yonyou.com",
  "MAIL_SMTP_PORT": 465,
  "MAIL_SMTP_SSL": true,
  "MAIL_SMTP_STARTTLS": false,
  "MAIL_USER": "zhangyshp@yonyou.com",
  "MAIL_PASS": "",
  "MAIL_IMAP_PASS": "",
  "MAIL_SMTP_PASS": "",
  "MAIL_PASSWORD_CMD": "",
  "MAIL_IMAP_PASSWORD_CMD": "security find-generic-password -a 'zhangyshp@yonyou.com' -s 'himalaya-imap' -w",
  "MAIL_SMTP_PASSWORD_CMD": "security find-generic-password -a 'zhangyshp@yonyou.com' -s 'himalaya-smtp' -w",
  "MAIL_INBOX": "INBOX",
  "MAIL_DEBUG": false
}
```

## 用法

### 配置检查

```bash
python3 skills/mail-ops/mail_ops.py config-check
```

输出当前生效配置，并对密码脱敏显示。

### 测试连接

```bash
python3 skills/mail-ops/mail_ops.py test-connection
python3 skills/mail-ops/mail_ops.py --debug test-connection
```

会分别测试：
- IMAP 连接与登录
- SMTP 连接与登录

### 列出邮箱文件夹

```bash
python3 skills/mail-ops/mail_ops.py folder-list
```

### 查看收件箱最新邮件

```bash
python3 skills/mail-ops/mail_ops.py list
python3 skills/mail-ops/mail_ops.py list --limit 20
python3 skills/mail-ops/mail_ops.py list --folder INBOX
```

### 搜索邮件

```bash
python3 skills/mail-ops/mail_ops.py search "会议"
python3 skills/mail-ops/mail_ops.py search "预算" --limit 20 --folder INBOX
```

### 读取正文

```bash
python3 skills/mail-ops/mail_ops.py read 12345
python3 skills/mail-ops/mail_ops.py read 12345 --folder INBOX
```

### 标记已读

```bash
python3 skills/mail-ops/mail_ops.py mark-read 12345
```

### 删除邮件

```bash
python3 skills/mail-ops/mail_ops.py delete 12345
```

### 发送邮件

```bash
python3 skills/mail-ops/mail_ops.py send \
  --to "a@example.com,b@example.com" \
  --subject "测试邮件" \
  --body "这是一封测试邮件"
```

## 参考 Himalaya 思路做的增强

本版相较于初版，增加了：
- `config-check`：输出当前配置摘要
- `test-connection`：独立测试 IMAP / SMTP 登录
- `folder-list`：列邮箱文件夹
- IMAP / SMTP 分离配置
- `SMTP SSL` 与 `SMTP STARTTLS` 可单独切换
- `MAIL_PASSWORD_CMD`：支持共用密码命令
- `MAIL_IMAP_PASSWORD_CMD`：支持单独读取 IMAP 密码
- `MAIL_SMTP_PASSWORD_CMD`：支持单独读取 SMTP 密码
- `--debug`：输出更详细调试信息
- 密码脱敏显示，避免日志泄露
- 与 Himalaya 的 Keychain 配置方式对齐

## 常见排查建议

如果出现登录失败：

1. 确认 `MAIL_USER` 是否正确
2. 确认是否与 Himalaya 使用的是同一个邮箱账号
3. 确认 Keychain 条目名称是否正确
4. 确认邮箱是否开启 IMAP / SMTP
5. 确认是否需要使用客户端授权码，而不是网页登录密码
6. 确认服务器地址、端口、SSL/STARTTLS 组合是否正确

常见组合示例：
- IMAP SSL：`993`
- SMTP SSL：`465`
- SMTP STARTTLS：`587`

## 当前能力

- `config-check`：检查当前配置
- `test-connection`：测试连接
- `folder-list`：列出邮箱文件夹
- `list`：列出最新邮件
- `search <keyword>`：搜索邮件
- `read <uid>`：读取邮件正文
- `send`：发送邮件
- `mark-read <uid>`：标记已读
- `delete <uid>`：删除邮件
