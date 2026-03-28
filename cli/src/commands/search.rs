use crate::api::ApiClient;
use colored::Colorize;

pub async fn run(client: &ApiClient, query: &str, agent_id: Option<&str>) -> Result<(), String> {
    let aid = if let Some(id) = agent_id {
        id.to_string()
    } else {
        let agents = client.list_agents().await?;
        agents["agents"].as_array().and_then(|a| a.first()).and_then(|a| a["id"].as_str())
            .map(String::from).ok_or("没有 Agent")?
    };

    let encoded = urlencoding::encode(query);
    let data = client.get(&format!("/api/v1/search/{}/{}", aid, encoded)).await?;
    let results = data["results"].as_array().ok_or("无结果")?;

    println!("{} results for \"{}\":", results.len().to_string().cyan().bold(), query);
    for r in results {
        let title = r["sessionTitle"].as_str().unwrap_or("?");
        let role = r["role"].as_str().unwrap_or("?");
        let snippet = r["snippet"].as_str().unwrap_or("");
        println!("  {} [{}] {}: {}", "•".green(), title.dimmed(), role.bold(), snippet);
    }
    Ok(())
}
