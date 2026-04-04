//! 共享辅助函数 — 被多个 handler 模块使用

use std::sync::atomic::{AtomicUsize, Ordering};
use std::collections::HashMap;
use std::sync::Mutex;
use once_cell::sync::Lazy;

use crate::agent;
use crate::db;

/// 全局 Key 轮换计数器（provider_id → 轮换索引）
static KEY_ROTATOR: Lazy<Mutex<HashMap<String, AtomicUsize>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// 从多 Key 字符串中轮换选择一个
///
/// 多个 Key 用 `|||` 分隔，例如 `sk-key1|||sk-key2|||sk-key3`。
/// 单 Key 时直接返回原值。
pub fn rotate_api_key(provider_id: &str, multi_key: &str) -> String {
    let keys: Vec<&str> = multi_key.split("|||").filter(|k| !k.trim().is_empty()).collect();
    if keys.len() <= 1 {
        return multi_key.to_string(); // 单 key 直接返回
    }

    let mut map = KEY_ROTATOR.lock().unwrap();
    let counter = map
        .entry(provider_id.to_string())
        .or_insert_with(|| AtomicUsize::new(0));
    let idx = counter.fetch_add(1, Ordering::Relaxed) % keys.len();
    keys[idx].trim().to_string()
}

/// 模型 → 上下文窗口大小映射（2026 年主流模型）
pub fn resolve_model_context_window(model: &str) -> usize {
    let m = model.to_lowercase();

    // ── OpenAI GPT 系列 ──
    // GPT-5.x 系列（2025-2026 最新）
    if m.contains("gpt-5") { return 1_000_000; }
    // GPT-4.x 系列
    if m.contains("gpt-4.1") || m.contains("gpt-4.5") { return 1_000_000; }
    if m.contains("gpt-4o") || m.contains("gpt-4-turbo") { return 128_000; }
    if m.contains("gpt-4") { return 128_000; }
    // o 系列推理模型
    if m.contains("o4-mini") || m.contains("o3-mini") { return 200_000; }
    if m.contains("o1") || m.contains("o3") || m.contains("o4") { return 200_000; }

    // ── Anthropic Claude 系列 ──
    // Claude 4.x / Opus 4.6 / Sonnet 4.6 / Haiku 4.5（最新一代，1M context）
    if m.contains("claude-opus-4") || m.contains("claude-sonnet-4") { return 1_000_000; }
    if m.contains("claude-haiku-4") { return 1_000_000; }
    // Claude 3.x 旧系列
    if m.contains("claude-3") { return 200_000; }
    // 通配
    if m.contains("claude") { return 1_000_000; }

    // ── Google Gemini 系列 ──
    if m.contains("gemini-2.5") || m.contains("gemini-2.0") { return 1_000_000; }
    if m.contains("gemini-1.5-pro") { return 2_000_000; }
    if m.contains("gemini") { return 1_000_000; }

    // ── DeepSeek 系列 ──
    if m.contains("deepseek-r1") || m.contains("deepseek-v3") { return 128_000; }
    if m.contains("deepseek") { return 128_000; }

    // ── Qwen 系列 ──
    if m.contains("qwen-long") || m.contains("qwen3") { return 1_000_000; }
    if m.contains("qwen") { return 128_000; }

    // ── xAI Grok 系列 ──
    if m.contains("grok-3") || m.contains("grok-4") { return 1_000_000; }
    if m.contains("grok") { return 131_072; }

    // ── Moonshot/Kimi 系列 ──
    if m.contains("moonshot") && m.contains("128k") { return 128_000; }
    if m.contains("moonshot") && m.contains("32k") { return 32_768; }
    if m.contains("kimi") || m.contains("moonshot") { return 128_000; }

    // ── 智谱 GLM 系列 ──
    if m.contains("glm-4") || m.contains("glm-5") { return 128_000; }
    if m.contains("glm") { return 128_000; }

    // ── Meta Llama 系列 ──
    if m.contains("llama-4") || m.contains("llama-3.3") { return 128_000; }
    if m.contains("llama") { return 128_000; }

    // ── Mistral 系列 ──
    if m.contains("mistral-large") || m.contains("mistral-medium") { return 128_000; }
    if m.contains("mistral") { return 32_768; }

    // ── MiniMax 系列 ──
    if m.contains("minimax") || m.contains("abab") { return 245_760; }

    // ── 百川 系列 ──
    if m.contains("baichuan") { return 128_000; }

    // ── 通用：名字中含 NNk/NNK ──
    if let Some(cap) = regex::Regex::new(r"(\d+)[kK]").ok().and_then(|re| re.captures(&m)) {
        if let Some(k) = cap.get(1).and_then(|m| m.as_str().parse::<usize>().ok()) {
            return k * 1_000;
        }
    }

    // 默认 200K（2026 年主流模型大多 ≥ 128K）
    200_000
}

/// 从数据库加载所有 provider 配置
pub async fn load_providers(db: &db::Database) -> Result<Vec<serde_json::Value>, String> {
    let json_str = db
        .get_setting("providers")
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "[]".to_string());
    serde_json::from_str(&json_str).map_err(|e| format!("解析 providers 配置失败: {}", e))
}

/// 保存所有 provider 配置到数据库
pub async fn save_providers(db: &db::Database, providers: &[serde_json::Value]) -> Result<(), String> {
    let json_str = serde_json::to_string(providers).map_err(|e| e.to_string())?;
    db.set_setting("providers", &json_str)
        .await
        .map_err(|e| format!("保存 providers 失败: {}", e))
}

/// 根据模型 ID 从 providers 中查找匹配的 provider 配置
///
/// 返回 (api_type, api_key, base_url)
/// 查找模型对应的 Provider（支持 `provider_id/model` 限定格式）
/// 同步版本（给非 async 调用方用）
pub fn find_provider_for_model(
    providers: &[serde_json::Value],
    model: &str,
) -> Option<(String, String, String)> {
    let (qualified_pid, model_id) = crate::channels::parse_qualified_model(model);

    // 第 0 轮：限定引用精确匹配
    if let Some(pid) = qualified_pid {
        for p in providers {
            if p["enabled"].as_bool() != Some(true) { continue; }
            if p["id"].as_str() == Some(pid) {
                let raw_key = p["apiKey"].as_str().unwrap_or("").to_string();
                if !raw_key.is_empty() {
                    let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
                    let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
                    let api_key = rotate_api_key(pid, &raw_key);
                    log::info!("find_provider_for_model: matched pid={}, api_type={}, base_url={}, key_len={}",
                        pid, api_type, base_url, api_key.len());
                    return Some((api_type, api_key, base_url));
                }
            }
        }
    }

    // 第一轮：按模型名匹配（大小写不敏感，兼容 MiniMax 等混合大小写模型）
    let model_id_lower = model_id.to_lowercase();
    for p in providers {
        if p["enabled"].as_bool() != Some(true) { continue; }
        let provider_id = p["id"].as_str().unwrap_or("unknown");
        if let Some(models) = p["models"].as_array() {
            for m in models {
                if m["id"].as_str().map(|s| s.to_lowercase()) == Some(model_id_lower.clone()) {
                    let raw_key = p["apiKey"].as_str().unwrap_or("").to_string();
                    if !raw_key.is_empty() {
                        let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
                        let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
                        let api_key = rotate_api_key(provider_id, &raw_key);
                        return Some((api_type, api_key, base_url));
                    }
                }
            }
        }
    }
    None
}

/// 确保 Agent 工作区已初始化
///
/// 如果 workspace_path 为 NULL（旧版本创建的 Agent），自动创建工作区并更新数据库
pub async fn ensure_agent_workspace(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
) -> Result<agent::AgentWorkspace, String> {
    let row = sqlx::query_as::<_, (Option<String>, String)>(
        "SELECT workspace_path, name FROM agents WHERE id = ?"
    )
    .bind(agent_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询失败: {}", e))?
    .ok_or("Agent 不存在")?;

    let (workspace_path, agent_name) = row;

    if let Some(wp) = workspace_path {
        // 检查是否为旧的 .openclaw 路径，自动迁移到 .xianzhu
        let wp = if wp.contains("/.openclaw/") {
            let new_wp = wp.replace("/.openclaw/", "/.xianzhu/");
            log::info!("迁移工作区路径: {} -> {}", wp, new_wp);
            // 如果旧目录存在，移动到新路径
            let old_path = std::path::PathBuf::from(&wp);
            let new_path = std::path::PathBuf::from(&new_wp);
            if old_path.exists() && !new_path.exists() {
                if let Some(parent) = new_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::rename(&old_path, &new_path) {
                    log::warn!("迁移工作区目录失败，将创建新目录: {}", e);
                }
            }
            // 更新数据库中的路径
            let _ = sqlx::query("UPDATE agents SET workspace_path = ? WHERE id = ?")
                .bind(&new_wp)
                .bind(agent_id)
                .execute(pool)
                .await;
            new_wp
        } else {
            wp
        };
        let ws = agent::AgentWorkspace::from_path(std::path::PathBuf::from(&wp), agent_id);
        // 确保目录也存在（可能被手动删除）
        if !ws.exists() {
            ws.initialize(&agent_name).await?;
        }
        Ok(ws)
    } else {
        // 旧 Agent，自动初始化工作区
        let ws = agent::AgentWorkspace::new(agent_id);
        ws.initialize(&agent_name).await?;
        let wp = ws.root().to_string_lossy().to_string();
        sqlx::query("UPDATE agents SET workspace_path = ? WHERE id = ?")
            .bind(&wp)
            .bind(agent_id)
            .execute(pool)
            .await
            .map_err(|e| format!("更新 workspace_path 失败: {}", e))?;
        log::info!("自动初始化 Agent {} 的工作区: {}", agent_id, wp);
        Ok(ws)
    }
}

/// 从 SKILL.md 内容解析元数据
pub fn parse_skill_meta(content: &str, default_name: &str) -> (String, String, Vec<String>) {
    let trimmed = content.trim();
    if trimmed.starts_with("---") {
        let rest = &trimmed[3..];
        if let Some(end) = rest.find("---") {
            let yaml_str = &rest[..end];
            if let Ok(data) = serde_yaml::from_str::<serde_json::Value>(yaml_str) {
                let name = data["name"].as_str().unwrap_or(default_name).to_string();
                let desc = data["description"].as_str().unwrap_or("").to_string();
                let tags = data["trigger_keywords"].as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                return (name, desc, tags);
            }
        }
    }
    // 纯 Markdown：从标题和首段推断
    let mut name = default_name.to_string();
    let mut desc = String::new();
    for line in trimmed.lines() {
        let l = line.trim();
        if l.starts_with("# ") && name == default_name {
            name = l.trim_start_matches("# ").to_string();
        } else if !l.is_empty() && !l.starts_with('#') && desc.is_empty() {
            desc = l.to_string();
            break;
        }
    }
    (name, desc, vec![])
}

/// 内置推荐技能（离线可用）
pub fn builtin_featured_skills() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"name": "web-search", "description": "Multi-engine web search (Brave/Exa/Tavily/DuckDuckGo)", "category": "search", "stars": 128, "installed": true}),
        serde_json::json!({"name": "code-review", "description": "AI-powered code review with best practices", "category": "development", "stars": 95}),
        serde_json::json!({"name": "git-helper", "description": "Git operations: commit, branch, merge, rebase", "category": "development", "stars": 87}),
        serde_json::json!({"name": "email-manager", "description": "Read/send/organize emails via IMAP/SMTP", "category": "productivity", "stars": 76}),
        serde_json::json!({"name": "calendar-sync", "description": "Read/create calendar events", "category": "productivity", "stars": 65}),
        serde_json::json!({"name": "database-query", "description": "Query SQLite/PostgreSQL/MySQL databases", "category": "data", "stars": 58}),
        serde_json::json!({"name": "api-tester", "description": "Test REST/GraphQL APIs with assertions", "category": "development", "stars": 52}),
        serde_json::json!({"name": "markdown-to-pdf", "description": "Convert Markdown documents to PDF", "category": "document", "stars": 45}),
        serde_json::json!({"name": "image-editor", "description": "Basic image operations (resize, crop, watermark)", "category": "media", "stars": 43}),
        serde_json::json!({"name": "translation", "description": "Multi-language translation with glossary", "category": "language", "stars": 41}),
    ]
}

/// 递归复制目录
pub fn copy_dir_recursive(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

/// 从应用内置资源释放 marketplace 技能
///
/// 检查 ~/.xianzhu/marketplace/ 是否为空或缺少技能，
/// 从 bundled-skills/ 资源释放到 marketplace 目录。
pub fn seed_marketplace_skills() {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return,
    };
    let marketplace_dir = home.join(".xianzhu/marketplace");
    let _ = std::fs::create_dir_all(&marketplace_dir);

    // 获取应用的 resource 目录（Tauri 打包后在 .app/Contents/Resources/）
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };

    // Tauri 1.x 资源路径：
    // macOS: XianZhu.app/Contents/Resources/bundled-skills/
    // Windows: <exe_dir>/bundled-skills/
    // Linux: <exe_dir>/bundled-skills/ 或 /usr/share/xianzhu/bundled-skills/
    let possible_paths = vec![
        exe_path.parent().unwrap_or(std::path::Path::new(".")).join("../Resources/bundled-skills"),
        exe_path.parent().unwrap_or(std::path::Path::new(".")).join("bundled-skills"),
        std::path::PathBuf::from("bundled-skills"), // 开发模式
    ];

    let bundled_dir = match possible_paths.iter().find(|p| p.exists()) {
        Some(p) => p.clone(),
        None => {
            log::info!("Marketplace: 未找到内置技能资源目录（开发模式正常）");
            return;
        }
    };

    // 遍历内置技能，缺失的释放到 marketplace
    let entries = match std::fs::read_dir(&bundled_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut seeded = 0;
    for entry in entries.flatten() {
        if !entry.path().is_dir() { continue; }
        let name = entry.file_name();
        let dest = marketplace_dir.join(&name);
        if !dest.exists() {
            if let Ok(_) = copy_dir_recursive(&entry.path(), &dest) {
                seeded += 1;
            }
        }
    }

    if seeded > 0 {
        log::info!("Marketplace: 已释放 {} 个内置技能到 {}", seeded, marketplace_dir.display());
    } else {
        let count = std::fs::read_dir(&marketplace_dir).map(|e| e.count()).unwrap_or(0);
        log::info!("Marketplace: {} 个技能已就绪", count);
    }
}

/// 自动安装技能的 CLI 依赖
///
/// 从 SKILL.md 的 frontmatter 解析 openclaw.requires.bins 和 openclaw.install，
/// 检测缺失的 CLI 工具并自动安装（brew/npm/pip）。
pub async fn auto_install_skill_deps(skill_name: &str, skill_md_content: &str) {
    let trimmed = skill_md_content.trim();
    if !trimmed.starts_with("---") { return; }
    let rest = &trimmed[3..];
    let end = match rest.find("---") { Some(e) => e, None => return };
    let yaml_str = &rest[..end];

    // 解析 YAML
    let data: serde_json::Value = match serde_yaml::from_str(yaml_str) {
        Ok(d) => d,
        Err(_) => return,
    };

    let meta = &data["metadata"]["openclaw"];
    let bins: Vec<String> = meta["requires"]["bins"].as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let installs = meta["install"].as_array().cloned().unwrap_or_default();

    if bins.is_empty() { return; }

    // 构建完整 PATH（包含 brew/npm/bun 路径）
    let home = dirs::home_dir().unwrap_or_default();
    let extra_path = format!(
        "/opt/homebrew/bin:/usr/local/bin:{}:{}:{}:{}",
        home.join(".xianzhu/runtime/node").to_string_lossy(),
        home.join(".npm-global/bin").to_string_lossy(),
        home.join(".bun/bin").to_string_lossy(),
        home.join(".local/bin").to_string_lossy(),
    );
    let full_path = format!("{}:{}", extra_path, std::env::var("PATH").unwrap_or_default());

    // 检测哪些 bin 缺失
    let mut missing: Vec<String> = Vec::new();
    for bin in &bins {
        let status = tokio::process::Command::new("which")
            .arg(bin)
            .env("PATH", &full_path)
            .output().await;
        if status.map(|o| !o.status.success()).unwrap_or(true) {
            missing.push(bin.clone());
        }
    }

    if missing.is_empty() {
        log::info!("技能 {}: 所有依赖已安装 ({:?})", skill_name, bins);
        return;
    }

    log::info!("技能 {}: 缺失依赖 {:?}，尝试自动安装...", skill_name, missing);

    // 找到捆绑的 Node/npm 路径
    let bundled_npm = find_bundled_npm(&home);

    // 按优先级排序安装方式：npm > brew > pip > cargo
    // 优先用我们捆绑的 npm，不依赖用户装 brew
    let mut installed = false;
    for install_item in &installs {
        if installed { break; }
        let kind = install_item["kind"].as_str().unwrap_or("");
        let result = match kind {
            "node" => {
                let package = install_item["package"].as_str().unwrap_or("");
                if package.is_empty() { continue; }
                // 用捆绑的 npm 安装
                let npm = bundled_npm.as_deref().unwrap_or("npm");
                run_install_cmd(skill_name, npm, &["install", "-g", package], &full_path).await
            }
            "brew" => {
                let formula = install_item["formula"].as_str().unwrap_or("");
                if formula.is_empty() { continue; }
                // 先检查 brew 是否存在
                let brew_exists = check_cmd_exists("brew", &full_path).await;
                if brew_exists {
                    run_install_cmd(skill_name, "brew", &["install", formula], &full_path).await
                } else {
                    // brew 不存在，尝试 npm 替代（很多 CLI 工具同时发布在 npm）
                    log::info!("技能 {}: brew 不存在，尝试 npm 安装 {}", skill_name, formula);
                    let npm = bundled_npm.as_deref().unwrap_or("npm");
                    let npm_result = run_install_cmd(skill_name, npm, &["install", "-g", formula], &full_path).await;
                    if !npm_result {
                        log::warn!("技能 {}: {} 需要 brew 安装但 brew 不可用，npm 安装也失败", skill_name, formula);
                    }
                    npm_result
                }
            }
            "pip" | "uv" => {
                let package = install_item["package"].as_str()
                    .or_else(|| install_item["args"].as_str())
                    .unwrap_or("");
                if package.is_empty() { continue; }
                if kind == "uv" && check_cmd_exists("uv", &full_path).await {
                    run_install_cmd(skill_name, "uv", &["tool", "install", package], &full_path).await
                } else {
                    run_install_cmd(skill_name, "pip3", &["install", "--user", package], &full_path).await
                }
            }
            "cargo" => {
                let crate_name = install_item["crate"].as_str().unwrap_or("");
                if crate_name.is_empty() { continue; }
                if check_cmd_exists("cargo", &full_path).await {
                    run_install_cmd(skill_name, "cargo", &["install", crate_name], &full_path).await
                } else {
                    log::warn!("技能 {}: cargo 不存在，无法安装 {}", skill_name, crate_name);
                    false
                }
            }
            _ => continue,
        };
        installed = result;
    }

    if !installed && !missing.is_empty() {
        log::warn!("技能 {}: 依赖 {:?} 自动安装失败，技能可能无法正常工作", skill_name, missing);
    }
}

/// 找到捆绑的 npm 路径
pub fn find_bundled_npm(home: &std::path::Path) -> Option<String> {
    let node_dir = home.join(".xianzhu/runtime/node");
    if !node_dir.exists() { return None; }
    let mut versions: Vec<_> = std::fs::read_dir(&node_dir).ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with("node-"))
        .collect();
    versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    versions.first().map(|v| {
        v.path().join("bin/npm").to_string_lossy().to_string()
    })
}

/// 检查命令是否存在
pub async fn check_cmd_exists(cmd: &str, path: &str) -> bool {
    tokio::process::Command::new("which")
        .arg(cmd)
        .env("PATH", path)
        .output().await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 执行安装命令，返回是否成功
pub async fn run_install_cmd(skill_name: &str, cmd: &str, args: &[&str], path: &str) -> bool {
    if !check_cmd_exists(cmd.split('/').last().unwrap_or(cmd), path).await {
        // 如果 cmd 是绝对路径，直接检查文件是否存在
        if !cmd.starts_with('/') || !std::path::Path::new(cmd).exists() {
            log::warn!("技能 {}: {} 不存在", skill_name, cmd);
            return false;
        }
    }
    log::info!("技能 {}: 执行 {} {:?}", skill_name, cmd, args);
    match tokio::process::Command::new(cmd)
        .args(args)
        .env("PATH", path)
        .output().await
    {
        Ok(output) => {
            if output.status.success() {
                log::info!("技能 {}: 安装成功 ({} {:?})", skill_name, cmd, args);
                true
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                log::warn!("技能 {}: 安装失败: {}", skill_name, stderr.chars().take(200).collect::<String>());
                false
            }
        }
        Err(e) => {
            log::warn!("技能 {}: 执行失败: {}", skill_name, e);
            false
        }
    }
}

/// 从云端下载技能到本地 marketplace（内部函数）
pub async fn download_skill_from_hub_inner(slug: &str) -> Result<String, String> {
    let marketplace_dir = dirs::home_dir()
        .ok_or("无法获取 home 目录")?
        .join(".xianzhu/marketplace");
    let dest = marketplace_dir.join(slug);

    if dest.exists() {
        return Ok(format!("技能 {} 已存在于本地 marketplace", slug));
    }

    let client = reqwest::Client::new();

    // 1. 先获取技能元数据
    let meta_url = format!("https://zys-openclaw.com/api/v1/skill-hub/{}", slug);
    let meta_resp = client.get(&meta_url).send().await
        .map_err(|e| format!("获取技能信息失败: {}", e))?;
    let meta: serde_json::Value = meta_resp.json().await
        .map_err(|e| format!("解析技能信息失败: {}", e))?;

    if meta.get("error").is_some() {
        return Err(format!("云端技能不存在: {}", slug));
    }

    // 2. 尝试下载技能包
    let download_url = format!("https://zys-openclaw.com/api/v1/skill-hub/{}/download", slug);
    let dl_resp = client.get(&download_url).send().await;

    let has_package = if let Ok(resp) = dl_resp {
        if resp.status().is_success() {
            let bytes = resp.bytes().await.map_err(|e| format!("下载失败: {}", e))?;
            let gz = flate2::read::GzDecoder::new(&bytes[..]);
            let mut archive = tar::Archive::new(gz);
            let _ = std::fs::create_dir_all(&dest);
            archive.unpack(&dest).map_err(|e| format!("解压失败: {}", e))?;
            true
        } else {
            false
        }
    } else {
        false
    };

    // 3. 如果没有包文件，从元数据生成一个基本的 SKILL.md
    if !has_package {
        let _ = std::fs::create_dir_all(&dest);
        let name = meta["name"].as_str().unwrap_or(slug);
        let desc = meta["description"].as_str().unwrap_or("");
        let _category = meta["category"].as_str().unwrap_or("general");
        let tags: Vec<String> = meta["tags"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let skill_md = format!(
            "---\nname: {}\ndescription: {}\ntrigger_keywords:\n{}\n---\n\n# {}\n\n{}\n",
            slug, desc,
            tags.iter().map(|t| format!("  - {}", t)).collect::<Vec<_>>().join("\n"),
            name, desc
        );
        std::fs::write(dest.join("SKILL.md"), skill_md)
            .map_err(|e| format!("写入 SKILL.md 失败: {}", e))?;
    }

    log::info!("云端技能下载完成: {} → {}", slug, dest.display());
    Ok(format!("技能 {} 已下载到本地", slug))
}
