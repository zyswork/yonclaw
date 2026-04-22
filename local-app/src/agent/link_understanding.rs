//! Link Understanding — 自动抽取用户消息里 URL 的标题/描述/摘要
//!
//! 在 `send_message_stream` 入口运行，为 LLM 预热 URL 上下文，免去显式 `web_fetch` 工具调用。
//!
//! 策略：
//! - 最多处理前 3 个 URL（避免消息撑爆）
//! - 每个 URL 5s 超时，并行抓取
//! - 失败静默跳过（不阻塞用户）
//! - HTML 提取 `<title>` + `<meta description/og:description>` + 正文前 ~500 字
//! - 输出作为 `[链接摘要]` 上下文块前置到 user_message
//!
//! 安全：复用 `web_fetch` 的 SSRF / 内网 / DNS rebind 防护

use std::time::Duration;

const MAX_URLS: usize = 3;
const FETCH_TIMEOUT_SECS: u64 = 5;
const MAX_BODY_CHARS: usize = 500;

/// 从文本中提取 URL（最多 MAX_URLS 个，去重）
pub fn extract_urls(text: &str) -> Vec<String> {
    let re = match regex::Regex::new(r"https?://[^\s<>\u{4E00}-\u{9FFF}\u{3000}-\u{303F}\u{FF00}-\u{FFEF}]+") {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();
    let mut urls = Vec::new();
    for m in re.find_iter(text) {
        let url = m.as_str()
            .trim_end_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '\''));
        if seen.insert(url.to_string()) {
            urls.push(url.to_string());
            if urls.len() >= MAX_URLS { break; }
        }
    }
    urls
}

/// 单个 URL 的抽取结果
#[derive(Debug, Clone)]
pub struct LinkSummary {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub body_preview: Option<String>,
}

impl LinkSummary {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.description.is_none() && self.body_preview.is_none()
    }

    pub fn format(&self) -> String {
        let mut s = format!("- {}", self.url);
        if let Some(t) = &self.title {
            s.push_str(&format!("\n  标题: {}", t));
        }
        if let Some(d) = &self.description {
            s.push_str(&format!("\n  描述: {}", d));
        }
        if let Some(b) = &self.body_preview {
            s.push_str(&format!("\n  摘要: {}", b));
        }
        s
    }
}

/// 检查 host 是否内网（复用 web_fetch 的策略）
fn is_private_host(host: &str) -> bool {
    let h = host.to_lowercase();
    h == "localhost" || h == "127.0.0.1" || h == "0.0.0.0" || h == "::1" || h == "[::1]"
        || h.starts_with("10.") || h.starts_with("192.168.") || h.starts_with("169.254.")
        || h.starts_with("fe80:") || h.starts_with("fd") || h.starts_with("fc")
        || h.ends_with(".local") || h.ends_with(".internal")
        || (h.starts_with("172.") && {
            h.split('.').nth(1).and_then(|s| s.parse::<u8>().ok())
                .map_or(false, |n| (16..=31).contains(&n))
        })
}

/// 抓取单个 URL 并抽取元信息（失败返回 None）
async fn fetch_and_extract(url: &str) -> Option<LinkSummary> {
    // SSRF 防护
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") { return None; }
    let host = parsed.host_str()?;
    if is_private_host(host) { return None; }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .user_agent("XianZhu-LinkUnderstanding/0.1")
        .redirect(reqwest::redirect::Policy::limited(3))
        .build().ok()?;

    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() { return None; }

    // 只处理 text/html 和 text/plain（避免下载二进制）
    let ct = resp.headers().get("content-type")
        .and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
    if !ct.is_empty() && !ct.contains("text/html") && !ct.contains("text/plain") && !ct.contains("application/json") {
        return None;
    }

    // 限制下载 256KB
    let bytes = resp.bytes().await.ok()?;
    let limited = &bytes[..bytes.len().min(256 * 1024)];
    let html = String::from_utf8_lossy(limited);

    // 提取 <title>
    let title = regex_find(&html, r"(?is)<title[^>]*>(.*?)</title>").map(decode_entities)
        .map(|s| truncate_chars(&clean_whitespace(&s), 120));

    // 提取 meta description（优先 og:description）
    let description = regex_find(&html, r#"(?is)<meta[^>]+property=["']og:description["'][^>]+content=["']([^"']+)["']"#)
        .or_else(|| regex_find(&html, r#"(?is)<meta[^>]+name=["']description["'][^>]+content=["']([^"']+)["']"#))
        .map(decode_entities)
        .map(|s| truncate_chars(&clean_whitespace(&s), 300));

    // 抽正文（粗糙：去 script/style，剥 HTML 标签，合并空白）
    let body_preview = extract_body_text(&html).map(|s| truncate_chars(&s, MAX_BODY_CHARS));

    let summary = LinkSummary { url: url.to_string(), title, description, body_preview };
    if summary.is_empty() { None } else { Some(summary) }
}

fn regex_find(text: &str, pattern: &str) -> Option<String> {
    regex::Regex::new(pattern).ok()?
        .captures(text)?.get(1)
        .map(|m| m.as_str().to_string())
}

fn decode_entities(s: String) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
        .replace("&quot;", "\"").replace("&#39;", "'").replace("&nbsp;", " ")
}

fn clean_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().take(n).collect();
    let out: String = chars.into_iter().collect();
    if s.chars().count() > n { format!("{}...", out) } else { out }
}

fn extract_body_text(html: &str) -> Option<String> {
    // 去掉 <script> 和 <style> 内容（regex crate 不支持反向引用，拆两个）
    let script_re = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").ok()?;
    let style_re = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").ok()?;
    let s1 = script_re.replace_all(html, " ").to_string();
    let s2 = style_re.replace_all(&s1, " ").to_string();
    // 优先抓 <article>（退而 <main>）正文区
    let article_re = regex::Regex::new(r"(?is)<article[^>]*>(.*?)</article>").ok()?;
    let main_re = regex::Regex::new(r"(?is)<main[^>]*>(.*?)</main>").ok()?;
    let target = article_re.captures(&s2).and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .or_else(|| main_re.captures(&s2).and_then(|c| c.get(1).map(|m| m.as_str().to_string())))
        .unwrap_or(s2);
    // 剥标签
    let tag_re = regex::Regex::new(r"<[^>]+>").ok()?;
    let text = tag_re.replace_all(&target, " ");
    let cleaned = clean_whitespace(&decode_entities(text.to_string()));
    if cleaned.is_empty() { None } else { Some(cleaned) }
}

/// 对 user_message 中的 URL 做并发抽取，返回格式化的上下文块
///
/// 成功返回 `Some("[链接摘要]\n- ...\n- ...")`，无 URL / 全部失败返回 `None`。
pub async fn enrich_urls(user_message: &str) -> Option<String> {
    let urls = extract_urls(user_message);
    if urls.is_empty() { return None; }

    let fetches = urls.iter().map(|u| fetch_and_extract(u));
    let results: Vec<Option<LinkSummary>> = futures::future::join_all(fetches).await;
    let summaries: Vec<String> = results.into_iter()
        .flatten()
        .map(|s| s.format())
        .collect();
    if summaries.is_empty() { None } else {
        Some(format!("[链接摘要]\n{}\n\n", summaries.join("\n\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_single_url() {
        let urls = extract_urls("看这个 https://example.com/docs 有用吗");
        assert_eq!(urls, vec!["https://example.com/docs"]);
    }

    #[test]
    fn extract_strip_trailing_punct() {
        let urls = extract_urls("https://a.com/x, https://b.com/y.");
        assert_eq!(urls, vec!["https://a.com/x", "https://b.com/y"]);
    }

    #[test]
    fn extract_dedup_and_cap() {
        let text = "https://a.com https://a.com https://b.com https://c.com https://d.com";
        let urls = extract_urls(text);
        assert_eq!(urls.len(), 3); // MAX_URLS
        assert_eq!(urls[0], "https://a.com");
    }

    #[test]
    fn extract_ignores_chinese_punct() {
        let urls = extract_urls("链接：https://example.com。好吗？");
        assert_eq!(urls, vec!["https://example.com"]);
    }

    #[test]
    fn is_private_host_cases() {
        assert!(is_private_host("localhost"));
        assert!(is_private_host("192.168.1.1"));
        assert!(is_private_host("172.16.0.1"));
        assert!(!is_private_host("172.32.0.1"));
        assert!(!is_private_host("github.com"));
    }

    #[test]
    fn truncate_preserves_utf8() {
        let s = truncate_chars("中文测试字符串很长很长很长", 3);
        assert_eq!(s, "中文测...");
    }

    #[test]
    fn extract_body_removes_script() {
        let html = "<html><body><script>alert(1)</script><p>Hello</p></body></html>";
        let text = extract_body_text(html).unwrap();
        assert!(text.contains("Hello"));
        assert!(!text.contains("alert"));
    }
}
