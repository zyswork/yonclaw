//! 一次性推理命令（参照 OpenClaw #62129 first-class infer CLI）
//!
//! 无需会话/Agent 上下文，直接让指定模型回答一个 prompt 并退出。
//! 适合脚本化用途：`xz infer --model gpt-4o "解释这段代码"`

use crate::api::ApiClient;
use colored::Colorize;

pub async fn run(
    client: &ApiClient,
    model: Option<&str>,
    prompt: &str,
    system: Option<&str>,
    json_output: bool,
) -> Result<(), String> {
    // 复用一个"临时 CLI Agent"
    let agents = client.list_agents().await?;
    let agent_list = agents["agents"].as_array().ok_or("无法获取 Agent 列表")?;
    if agent_list.is_empty() {
        return Err("没有 Agent。请先在桌面端创建一个。".into());
    }

    // 优先挑同模型的 agent；找不到就用第一个
    let agent = if let Some(m) = model {
        agent_list.iter().find(|a| a["model"].as_str() == Some(m))
            .unwrap_or(&agent_list[0])
    } else {
        &agent_list[0]
    };

    let aid = agent["id"].as_str().unwrap_or("");
    let used_model = agent["model"].as_str().unwrap_or("?");

    // 使用临时 session ID（带时间戳避免碰撞）
    let sid = format!("cli-infer-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );

    // 拼接输入
    let full_prompt = match system {
        Some(s) => format!("[系统指令] {}\n\n{}", s, prompt),
        None => prompt.to_string(),
    };

    if !json_output {
        eprintln!("{} model={} prompt={} chars", "Inferring".cyan(), used_model, prompt.len());
    }

    let resp = client.send_message(aid, &sid, &full_prompt).await?;
    let reply = resp["response"].as_str().unwrap_or("").trim();

    if json_output {
        let out = serde_json::json!({
            "model": used_model,
            "prompt_chars": prompt.len(),
            "reply": reply,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    } else {
        println!("{}", reply);
    }
    Ok(())
}
