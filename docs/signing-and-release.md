# 签名、公证与自动更新

本文档描述 XianZhu 发布流程所需的凭证和配置步骤。

## 一、Tauri Updater 签名密钥

Updater 使用 Ed25519 密钥对签名新版本，客户端用公钥验证。

### 生成密钥对

```bash
cd local-app
cargo tauri signer generate -w ~/.tauri/xianzhu.key
# 输入密码后生成：
# ~/.tauri/xianzhu.key         — 私钥（不要提交到 git）
# ~/.tauri/xianzhu.key.pub     — 公钥
```

### 配置

1. 把 **公钥内容** 填入 `local-app/tauri.conf.json`：
   ```json
   "updater": {
     "active": true,
     "endpoints": ["https://github.com/<owner>/<repo>/releases/latest/download/latest.json"],
     "dialog": true,
     "pubkey": "<公钥内容>"
   }
   ```

2. 在 GitHub Secrets 添加：
   - `TAURI_PRIVATE_KEY` — 私钥文件的 base64 内容
   - `TAURI_KEY_PASSWORD` — 生成时设置的密码

### 本地构建时签名

```bash
export TAURI_PRIVATE_KEY=$(base64 -i ~/.tauri/xianzhu.key)
export TAURI_KEY_PASSWORD="<密码>"
cargo tauri build
```

签名成功后会产出 `XianZhuClaw.app.tar.gz.sig`，上传到 GitHub Release。

---

## 二、macOS 代码签名 + 公证

未签名的 DMG 在用户端会被 Gatekeeper 拦截（"无法验证开发者"）。

### 前置条件

- Apple Developer Program 会员（每年 $99）
- Developer ID Application 证书

### 导出证书

1. Keychain Access → Developer ID Application → 右键 Export → `.p12` → 记住密码
2. `base64 -i cert.p12 | pbcopy` → 粘贴到 GitHub Secret `APPLE_CERTIFICATE`
3. 证书密码 → Secret `APPLE_CERTIFICATE_PASSWORD`

### 公证凭证（notarytool）

1. 登录 https://appleid.apple.com → 生成 app-specific password
2. GitHub Secrets 添加：
   - `APPLE_ID` — Apple ID 邮箱
   - `APPLE_PASSWORD` — app-specific password（不是登录密码）
   - `APPLE_TEAM_ID` — 10 位 Team ID（Apple Developer 账号首页查看）
   - `APPLE_SIGNING_IDENTITY` — 形如 `"Developer ID Application: Your Name (TEAM_ID)"`

### tauri.conf.json 配置

```json
"macOS": {
  "signingIdentity": "-",
  "hardenedRuntime": true,
  "entitlements": "entitlements.plist"
}
```

构建时 `cargo tauri build` 会自动读取环境变量签名 + 公证。

---

## 三、Windows 签名（可选）

### 前置条件

- Code Signing 证书（DigiCert / Sectigo / SSL.com，约 $300-500/年）

### GitHub Secrets

- `WINDOWS_CERTIFICATE` — `.pfx` base64
- `WINDOWS_CERTIFICATE_PASSWORD`

### tauri.conf.json

```json
"windows": {
  "certificateThumbprint": null,
  "digestAlgorithm": "sha256",
  "timestampUrl": "http://timestamp.digicert.com"
}
```

---

## 四、发布流程

### 手动发布

```bash
# 1. 更新版本号
sed -i '' 's/"version": ".*"/"version": "1.1.0"/' local-app/tauri.conf.json

# 2. 更新 CHANGELOG.md

# 3. 打 tag 触发 CI
git tag v1.1.0
git push origin v1.1.0

# 4. GitHub Actions 自动构建 macOS / Windows / Linux 产物，创建 Draft Release
# 5. 在 Release 页面编辑发布说明，发布
```

### 手动本地发布（应急）

```bash
cd local-app
cargo tauri build --target aarch64-apple-darwin
cargo tauri build --target x86_64-apple-darwin
# 产物在 target/{target}/release/bundle/dmg/
```

---

## 五、更新 latest.json

GitHub Release 发布后手工或脚本生成：

```json
{
  "version": "1.1.0",
  "notes": "See CHANGELOG.md",
  "pub_date": "2026-04-17T00:00:00Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<sig 文件内容>",
      "url": "https://github.com/<owner>/<repo>/releases/download/v1.1.0/XianZhuClaw_1.1.0_aarch64.app.tar.gz"
    },
    "darwin-x86_64": { ... },
    "windows-x86_64": { ... },
    "linux-x86_64": { ... }
  }
}
```

上传到 Release 作为 `latest.json` 资产，客户端 updater 会自动拉取。

---

## 常见问题

- **DMG 未签名直接开？** 用户首次打开可能需 `System Preferences → Security & Privacy → Open Anyway`。
- **Notarization 失败？** 检查 `hardenedRuntime: true` 和 `entitlements.plist`。
- **Updater 未弹窗？** 确认 `tauri.conf.json` 的 `updater.active=true` 且 `endpoints` URL 可达。
