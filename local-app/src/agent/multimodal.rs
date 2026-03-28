//! 多模态消息处理
//!
//! 支持在消息中传递图片（URL 或 base64），
//! 根据模型能力自动适配格式。
//! 借鉴 ZeroClaw 的 multimodal 模块。

/// 多模态配置
#[derive(Debug, Clone)]
pub struct MultimodalConfig {
    /// 最大图片数（per message）
    pub max_images: usize,
    /// 最大图片大小（字节）
    pub max_image_size: usize,
    /// 支持的图片格式
    pub allowed_formats: Vec<String>,
}

impl Default for MultimodalConfig {
    fn default() -> Self {
        Self {
            max_images: 5,
            max_image_size: 20 * 1024 * 1024, // 20MB
            allowed_formats: vec!["png".into(), "jpg".into(), "jpeg".into(), "gif".into(), "webp".into()],
        }
    }
}

/// 检测消息中的图片标记
///
/// 支持 markdown 图片语法: ![alt](url)
/// 和自定义标记: [image:url] 或 [image:base64:data]
pub fn extract_image_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();

    // Markdown 图片: ![...](url)
    let md_re = regex::Regex::new(r"!\[[^\]]*\]\(([^)]+)\)").unwrap();
    for cap in md_re.captures_iter(text) {
        if let Some(url) = cap.get(1) {
            urls.push(url.as_str().to_string());
        }
    }

    // 自定义标记: [image:url]
    let custom_re = regex::Regex::new(r"\[image:(https?://[^\]]+)\]").unwrap();
    for cap in custom_re.captures_iter(text) {
        if let Some(url) = cap.get(1) {
            urls.push(url.as_str().to_string());
        }
    }

    // Base64 data URL: data:image/xxx;base64,...
    let data_re = regex::Regex::new(r"(data:image/[a-zA-Z]+;base64,[A-Za-z0-9+/=]+)").unwrap();
    for cap in data_re.captures_iter(text) {
        if let Some(url) = cap.get(1) {
            urls.push(url.as_str().to_string());
        }
    }

    // 附件标记: [attachment:base64:data]（前端发送的格式）
    let attach_re = regex::Regex::new(r"\[attachment:([^\]]+)\]").unwrap();
    for cap in attach_re.captures_iter(text) {
        if let Some(data) = cap.get(1) {
            let s = data.as_str();
            if s.starts_with("data:image/") {
                urls.push(s.to_string());
            }
        }
    }

    urls
}

/// 把 base64 图片保存到磁盘（~/.xianzhu/media/），返回保存后的文件路径列表
pub fn save_images_to_disk(image_urls: &[String], agent_id: &str) -> Vec<String> {
    let media_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".xianzhu")
        .join("media");
    let _ = std::fs::create_dir_all(&media_dir);

    let mut saved = Vec::new();
    for url in image_urls {
        if !url.starts_with("data:image/") { continue; }

        // 解析 data:image/jpeg;base64,xxxxx
        let parts: Vec<&str> = url.splitn(2, ',').collect();
        if parts.len() != 2 { continue; }

        let ext = if parts[0].contains("png") { "png" }
            else if parts[0].contains("webp") { "webp" }
            else { "jpg" };

        // 标准库解码 base64（用 data_encoding 兼容方式）
        let b64_clean: String = parts[1].chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = match decode_base64(&b64_clean) {
            Some(b) => b,
            None => continue,
        };

        let id_prefix = if agent_id.len() >= 8 { &agent_id[..8] } else { agent_id };
        let filename = format!("{}-{}.{}", id_prefix, uuid::Uuid::new_v4(), ext);
        let path = media_dir.join(&filename);

        match std::fs::write(&path, &bytes) {
            Ok(_) => {
                log::info!("图片已保存: {} ({}KB)", path.display(), bytes.len() / 1024);
                saved.push(path.to_string_lossy().to_string());
            }
            Err(e) => log::warn!("图片保存失败: {}", e),
        }
    }
    saved
}

/// 简单 base64 解码（不依赖外部 crate）
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &ch in input.as_bytes() {
        let val = if ch == b'=' { break; }
        else if let Some(pos) = TABLE.iter().position(|&c| c == ch) { pos as u32 }
        else { continue; };
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    if output.is_empty() { None } else { Some(output) }
}

/// 从消息文本中移除图片标记，返回纯文本
pub fn strip_image_markers(text: &str) -> String {
    let mut result = text.to_string();
    // 移除 [attachment:...]
    let attach_re = regex::Regex::new(r"\[attachment:[^\]]+\]\s*").unwrap();
    result = attach_re.replace_all(&result, "").to_string();
    // 移除内联 base64 data URL（太长）
    let data_re = regex::Regex::new(r"data:image/[a-zA-Z]+;base64,[A-Za-z0-9+/=]+").unwrap();
    result = data_re.replace_all(&result, "[图片]").to_string();
    result.trim().to_string()
}

/// 将含图片的消息转换为 OpenAI vision 格式
///
/// OpenAI: content 变为数组 [{"type":"text","text":"..."}, {"type":"image_url","image_url":{"url":"..."}}]
pub fn to_vision_message(role: &str, text: &str, image_urls: &[String]) -> serde_json::Value {
    if image_urls.is_empty() {
        return serde_json::json!({"role": role, "content": text});
    }

    let mut content = vec![
        serde_json::json!({"type": "text", "text": text})
    ];

    for url in image_urls {
        content.push(serde_json::json!({
            "type": "image_url",
            "image_url": {"url": url, "detail": "auto"}
        }));
    }

    serde_json::json!({"role": role, "content": content})
}

/// 检查模型是否支持 vision
pub fn supports_vision(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("gpt-4o") || m.contains("gpt-4-turbo") || m.contains("gpt-4-vision")
        || m.contains("claude-3") || m.contains("claude-sonnet-4") || m.contains("claude-opus-4")
        || m.contains("gemini")
        || m.starts_with("gpt-5")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_md_images() {
        let text = "看这张图 ![photo](https://example.com/img.png) 怎么样";
        let urls = extract_image_urls(text);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://example.com/img.png");
    }

    #[test]
    fn test_extract_custom_images() {
        let text = "分析 [image:https://example.com/chart.jpg]";
        let urls = extract_image_urls(text);
        assert_eq!(urls.len(), 1);
    }

    #[test]
    fn test_no_images() {
        let urls = extract_image_urls("普通文本");
        assert!(urls.is_empty());
    }

    #[test]
    fn test_vision_message() {
        let msg = to_vision_message("user", "描述这张图", &["https://img.com/a.png".into()]);
        assert!(msg["content"].is_array());
        assert_eq!(msg["content"][0]["type"], "text");
        assert_eq!(msg["content"][1]["type"], "image_url");
    }

    #[test]
    fn test_supports_vision() {
        assert!(supports_vision("gpt-4o"));
        assert!(supports_vision("claude-sonnet-4-20250514"));
        assert!(!supports_vision("deepseek-chat"));
    }
}
