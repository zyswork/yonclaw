# Updater Fallback

Tauri updater 使用**多 endpoint 容错**。首选走 `zys-openclaw.com`，主域名失效时降级走这里的 `latest.json`（通过 `raw.githubusercontent.com` 静态分发）。

## 文件

- `latest.json` — 版本元数据 + 各平台下载 URL + 签名

## 发布流程

每次 release：

1. `cargo tauri build` 在 `local-app/target/release/bundle/{macos,msi,appimage}/` 产出 bundle 和 `.sig` 签名文件
2. 上传 bundle（`.app.tar.gz` / `.msi.zip` / `.AppImage.tar.gz`）到 GitHub Releases
3. 读取对应的 `.sig` 文件内容（单行 base64）填入 `latest.json` 的 `signature` 字段
4. 更新 `version` 和 `pub_date`
5. `git commit` 推到 main 分支 → `raw.githubusercontent.com` 立即生效

## 一键更新脚本

```bash
# 在 my-openclaw/ 根目录
VERSION=1.1.1
SIG_MAC_ARM=$(cat local-app/target/release/bundle/macos/XianZhuClaw.app.tar.gz.sig)
# ... 其他平台类推

# 用 jq 或手改 updater/latest.json
jq --arg v "$VERSION" --arg sig "$SIG_MAC_ARM" \
  '.version = $v | .pub_date = (now | todate) | .platforms."darwin-aarch64".signature = $sig' \
  updater/latest.json > updater/latest.tmp && mv updater/latest.tmp updater/latest.json

git add updater/latest.json && git commit -m "chore(updater): release $VERSION" && git push
```

## 为什么 fallback 重要

- 主域名 `zys-openclaw.com` 挂 / 备案问题 / 服务器迁移 → 用户永远停在旧版
- GitHub raw 分发免费 + 全球 CDN + 免维护
- Tauri 1.x 原生支持按顺序 try endpoints，第一个成功就用

## 注意

- **不能把私钥 `.key` commit 进来**
- 修改 `latest.json` 的 version 字段必须和 `tauri.conf.json`、`Cargo.toml`、`package.json` 一致
- `signature` 字段必须是**对应平台 `.sig` 文件**的内容，不是 pubkey
