//! 技能管理命令

use std::sync::Arc;
use tauri::State;

use crate::agent;
use crate::AppState;
use super::helpers::{
    ensure_agent_workspace, copy_dir_recursive, auto_install_skill_deps,
    download_skill_from_hub_inner, parse_skill_meta, builtin_featured_skills,
};

/// 安装技能
#[tauri::command]
pub async fn install_skill(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    file_path: String,
) -> Result<serde_json::Value, String> {
    // H9: 路径安全校验
    if file_path.contains("..") {
        return Err("路径包含非法遍历序列".to_string());
    }
    let src_path = std::path::Path::new(&file_path);
    let canonical = src_path.canonicalize()
        .map_err(|e| format!("路径规范化失败: {}", e))?;
    let path_str = canonical.to_string_lossy();
    if path_str.starts_with("/etc") || path_str.starts_with("/usr") || path_str.starts_with("/System") || path_str.starts_with("/bin") || path_str.starts_with("/sbin") {
        return Err("安全限制：不允许从系统路径安装技能".to_string());
    }

    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;
    let skills_dir = workspace.root().join("skills");
    let mut skill_mgr = agent::SkillManager::scan(&skills_dir);

    let manifest = skill_mgr.install_from_file(
        &canonical,
        &agent_id,
        state.orchestrator.pool(),
    ).await?;

    Ok(serde_json::json!({
        "name": manifest.name,
        "version": manifest.version,
        "description": manifest.description,
        "tools_count": manifest.tools.len(),
    }))
}

/// 移除技能
#[tauri::command]
pub async fn remove_skill(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    skill_name: String,
) -> Result<(), String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;
    let skills_dir = workspace.root().join("skills");
    let mut skill_mgr = agent::SkillManager::scan(&skills_dir);

    skill_mgr.remove_skill(&skill_name, &agent_id, state.orchestrator.pool()).await
}

/// 列出已安装的技能（合并数据库记录 + 文件系统扫描）
#[tauri::command]
pub async fn list_skills(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let mut db_skills = agent::SkillManager::list_installed(&agent_id, state.orchestrator.pool()).await?;
    let db_names: std::collections::HashSet<String> = db_skills.iter()
        .filter_map(|s| s.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
        .collect();

    if let Ok(workspace) = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await {
        let skills_dir = workspace.root().join("skills");
        let fs_manager = agent::SkillManager::scan(&skills_dir);
        for skill in fs_manager.index() {
            if !db_names.contains(&skill.name) {
                db_skills.push(serde_json::json!({
                    "id": format!("fs-{}", skill.name),
                    "name": skill.name,
                    "version": "",
                    "enabled": true,
                    "installed_at": "",
                    "tools_count": 0,
                    "description": skill.description,
                    "source": "filesystem",
                }));
            }
        }
    }

    Ok(db_skills)
}

/// 切换技能启用状态
#[tauri::command]
pub async fn toggle_skill(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    skill_name: String,
    enabled: bool,
) -> Result<(), String> {
    agent::SkillManager::toggle_skill(&skill_name, &agent_id, enabled, state.orchestrator.pool()).await
}

/// 列出技能市场中的所有可用技能
#[tauri::command]
pub async fn list_marketplace_skills() -> Result<Vec<serde_json::Value>, String> {
    let marketplace_dir = dirs::home_dir()
        .ok_or("无法获取 home 目录")?
        .join(".xianzhu/marketplace");

    if !marketplace_dir.exists() {
        return Ok(Vec::new());
    }

    let mgr = agent::SkillManager::scan(&marketplace_dir);
    let mut result = Vec::new();
    for skill in mgr.index() {
        let manifest = mgr.get_manifest(&skill.name);
        let tools_count = manifest.map_or(0, |m| m.tools.len());
        result.push(serde_json::json!({
            "name": skill.name,
            "dir_name": if skill.dir_name.is_empty() { &skill.name } else { &skill.dir_name },
            "description": skill.description,
            "tools_count": tools_count,
            "trigger_keywords": skill.trigger_keywords,
        }));
    }
    Ok(result)
}

/// 搜索云端技能市场
#[tauri::command]
pub async fn search_skill_hub(
    query: String,
) -> Result<Vec<serde_json::Value>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build().map_err(|e| e.to_string())?;

    let url = format!("https://zys-openclaw.com/api/v1/skill-hub/search?q={}", urlencoding::encode(&query));
    let resp = client.get(&url).send().await
        .map_err(|e| format!("搜索失败: {}", e))?;

    if !resp.status().is_success() {
        return Ok(Vec::new());
    }

    let data: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!([]));
    if let Some(arr) = data.as_array() {
        Ok(arr.clone())
    } else if let Some(arr) = data["results"].as_array() {
        Ok(arr.clone())
    } else {
        Ok(Vec::new())
    }
}

/// 从云端技能市场下载并安装到本地 marketplace（Tauri command）
#[tauri::command]
pub async fn download_skill_from_hub(slug: String) -> Result<String, String> {
    download_skill_from_hub_inner(&slug).await
}

/// 安装技能到指定 Agent（从 marketplace 复制到 agent skills 目录）
#[tauri::command]
pub async fn install_skill_to_agent(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    skill_name: String,
) -> Result<String, String> {
    let marketplace_dir = dirs::home_dir()
        .ok_or("无法获取 home 目录")?
        .join(".xianzhu/marketplace");
    let src = marketplace_dir.join(&skill_name);

    if !src.exists() {
        log::info!("技能 {} 不在本地，尝试从云端下载...", skill_name);
        match download_skill_from_hub_inner(&skill_name).await {
            Ok(msg) => log::info!("技能下载完成: {}", msg),
            Err(e) => return Err(format!("技能不存在或下载失败: {}", e)),
        }
        if !src.exists() {
            return Err(format!("技能下载后仍未找到: {}", skill_name));
        }
    }

    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;
    let dest = workspace.root().join("skills").join(&skill_name);

    if dest.exists() {
        return Err(format!("技能已安装: {}", skill_name));
    }

    let _ = std::fs::create_dir_all(workspace.root().join("skills"));
    copy_dir_recursive(&src, &dest).map_err(|e| format!("复制技能失败: {}", e))?;

    // 自动安装 CLI 依赖
    let skill_md = src.join("SKILL.md");
    if skill_md.exists() {
        if let Ok(content) = std::fs::read_to_string(&skill_md) {
            auto_install_skill_deps(&skill_name, &content).await;
        }
    }

    state.orchestrator.invalidate_skill_cache();

    log::info!("技能已安装: {} -> agent {}（缓存已失效，下次对话立即生效）", skill_name, agent_id);

    // 检测是否有 .example 配置文件需要用户手动配置
    let mut setup_hints = Vec::new();
    let example_files = ["cookie.txt.example", "config.txt.example", ".env.example", "token.txt.example"];
    for ef in &example_files {
        if dest.join(ef).exists() {
            let target_name = ef.trim_end_matches(".example");
            if !dest.join(target_name).exists() {
                setup_hints.push(format!("请配置 {}", target_name));
            }
        }
    }
    if let Ok(content) = std::fs::read_to_string(dest.join("SKILL.md")) {
        if content.contains("oa-common") && skill_name != "oa-common" {
            let common_dir = workspace.root().join("skills/oa-common");
            if !common_dir.exists() {
                setup_hints.push("依赖技能 oa-公共层 未安装，请先安装".to_string());
            }
        }
    }

    if setup_hints.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("安装成功！配置提示：{}", setup_hints.join("；")))
    }
}

/// 从 Agent 卸载技能
#[tauri::command]
pub async fn uninstall_skill_from_agent(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    skill_name: String,
) -> Result<(), String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;
    let skill_dir = workspace.root().join("skills").join(&skill_name);

    if !skill_dir.exists() {
        return Err(format!("技能未安装: {}", skill_name));
    }

    std::fs::remove_dir_all(&skill_dir)
        .map_err(|e| format!("卸载技能失败: {}", e))?;

    state.orchestrator.invalidate_skill_cache();

    log::info!("技能已卸载: {} from agent {}（缓存已失效，下次对话立即生效）", skill_name, agent_id);
    Ok(())
}

/// 将本地技能发布到云端技能市场
#[tauri::command]
pub async fn publish_skill_to_hub(
    skill_name: String,
    author: String,
) -> Result<String, String> {
    let marketplace_dir = dirs::home_dir()
        .ok_or("无法获取 home 目录")?
        .join(".xianzhu/marketplace");
    let skill_dir = marketplace_dir.join(&skill_name);

    if !skill_dir.exists() {
        return Err(format!("技能不存在: {}", skill_name));
    }

    let skill_md_path = skill_dir.join("SKILL.md");
    if !skill_md_path.exists() {
        return Err("缺少 SKILL.md".to_string());
    }

    let content = std::fs::read_to_string(&skill_md_path)
        .map_err(|e| format!("读取 SKILL.md 失败: {}", e))?;

    let (name, description, tags) = parse_skill_meta(&content, &skill_name);

    let client = reqwest::Client::new();

    let publish_url = "https://zys-openclaw.com/api/v1/skill-hub/publish";
    let resp = client.post(publish_url)
        .json(&serde_json::json!({
            "slug": skill_name,
            "name": name,
            "description": description,
            "author": if author.is_empty() { "community".to_string() } else { author },
            "version": "1.0.0",
            "category": "community",
            "tags": tags,
        }))
        .send().await
        .map_err(|e| format!("发布失败: {}", e))?;

    let result: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    if result.get("error").is_some() {
        return Err(format!("发布失败: {}", result["error"].as_str().unwrap_or("?")));
    }

    // 打包并上传技能包（tar.gz）
    let tar_path = std::env::temp_dir().join(format!("{}.tar.gz", skill_name));
    {
        let tar_file = std::fs::File::create(&tar_path)
            .map_err(|e| format!("创建打包文件失败: {}", e))?;
        let enc = flate2::write::GzEncoder::new(tar_file, flate2::Compression::default());
        let mut tar_builder = tar::Builder::new(enc);

        let excluded = ["cookie.txt", "config.txt", ".env", "token.txt", "credentials.json"];

        fn add_dir_filtered(builder: &mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>, dir: &std::path::Path, prefix: &std::path::Path, excluded: &[&str]) -> Result<(), String> {
            for entry in std::fs::read_dir(dir).map_err(|e| format!("读取目录失败: {}", e))? {
                let entry = entry.map_err(|e| format!("读取条目失败: {}", e))?;
                let path = entry.path();
                let file_name = entry.file_name().to_string_lossy().to_string();

                if excluded.iter().any(|&e| file_name == e) {
                    log::info!("发布跳过敏感文件: {}", file_name);
                    continue;
                }

                let archive_name = prefix.join(&file_name);
                if path.is_dir() {
                    add_dir_filtered(builder, &path, &archive_name, excluded)?;
                } else {
                    builder.append_path_with_name(&path, &archive_name)
                        .map_err(|e| format!("添加文件失败: {}", e))?;
                }
            }
            Ok(())
        }

        add_dir_filtered(&mut tar_builder, &skill_dir, std::path::Path::new("."), &excluded)
            .map_err(|e| format!("打包失败: {}", e))?;
        tar_builder.finish().map_err(|e| format!("完成打包失败: {}", e))?;
    }

    let tar_bytes = std::fs::read(&tar_path)
        .map_err(|e| format!("读取打包文件失败: {}", e))?;
    let upload_url = format!("https://zys-openclaw.com/api/v1/skill-hub/{}/upload", skill_name);
    let _ = client.post(&upload_url)
        .header("Content-Type", "application/octet-stream")
        .body(tar_bytes)
        .send().await;

    let _ = std::fs::remove_file(&tar_path);

    log::info!("技能已发布到云端: {}", skill_name);
    Ok(format!("技能 {} 已发布", skill_name))
}

/// ClawHub: 热门/推荐技能
#[tauri::command]
pub async fn clawhub_featured() -> Result<Vec<serde_json::Value>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build().map_err(|e| e.to_string())?;

    let resp = client.get("https://zys-openclaw.com/api/v1/skill-hub/featured")
        .send().await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let data: serde_json::Value = r.json().await.unwrap_or(serde_json::json!([]));
            if let Some(arr) = data.as_array() {
                Ok(arr.clone())
            } else if let Some(arr) = data["skills"].as_array() {
                Ok(arr.clone())
            } else {
                Ok(builtin_featured_skills())
            }
        }
        _ => {
            Ok(builtin_featured_skills())
        }
    }
}

/// ClawHub: 技能分类列表
#[tauri::command]
pub async fn clawhub_categories() -> Result<Vec<serde_json::Value>, String> {
    Ok(vec![
        serde_json::json!({"id": "search", "name": "Search", "icon": "\u{1f50d}", "count": 6}),
        serde_json::json!({"id": "development", "name": "Development", "icon": "\u{1f4bb}", "count": 15}),
        serde_json::json!({"id": "productivity", "name": "Productivity", "icon": "\u{1f4cb}", "count": 12}),
        serde_json::json!({"id": "data", "name": "Data & Analytics", "icon": "\u{1f4ca}", "count": 8}),
        serde_json::json!({"id": "document", "name": "Document", "icon": "\u{1f4c4}", "count": 7}),
        serde_json::json!({"id": "media", "name": "Media", "icon": "\u{1f3a8}", "count": 5}),
        serde_json::json!({"id": "language", "name": "Language", "icon": "\u{1f310}", "count": 6}),
        serde_json::json!({"id": "automation", "name": "Automation", "icon": "\u{26a1}", "count": 10}),
        serde_json::json!({"id": "security", "name": "Security", "icon": "\u{1f512}", "count": 4}),
        serde_json::json!({"id": "communication", "name": "Communication", "icon": "\u{1f4ac}", "count": 8}),
    ])
}

/// ClawHub: 安装技能（从市场下载到本地）
#[tauri::command]
pub async fn clawhub_install(slug: String, agent_id: Option<String>) -> Result<String, String> {
    let result = download_skill_from_hub_inner(&slug).await?;

    if let Some(aid) = agent_id {
        log::info!("ClawHub: 自动安装 {} 到 agent {}", slug, aid);
    }

    Ok(result)
}
