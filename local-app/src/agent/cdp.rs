//! Chrome DevTools Protocol (CDP) 客户端
//!
//! 通过 WebSocket 与 Chrome/Chromium 浏览器通信，
//! 支持页面导航、截图、ARIA 快照、JS 执行等。
//!
//! 参考 OpenClaw src/browser/cdp.ts 实现。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};

/// CDP 连接
pub struct CdpClient {
    /// WebSocket 写入端
    writer: Arc<Mutex<futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        tokio_tungstenite::tungstenite::Message
    >>>,
    /// 请求 ID 计数器
    next_id: AtomicU64,
    /// 待响应请求
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>,
}

/// CDP 远程对象
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpRemoteObject {
    #[serde(rename = "type")]
    pub obj_type: String,
    pub value: Option<serde_json::Value>,
    pub description: Option<String>,
}

/// 浏览器 Tab 信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: String,
    pub title: String,
    pub url: String,
    #[serde(rename = "type")]
    pub tab_type: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    pub ws_url: Option<String>,
}

/// ARIA 快照节点
#[derive(Debug, Clone, Serialize)]
pub struct AriaNode {
    pub ref_id: String,
    pub role: String,
    pub name: String,
    pub value: Option<String>,
    pub depth: usize,
    /// CDP Accessibility 节点的 backendDOMNodeId（用于 ref 定位）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_node_id: Option<i64>,
}

impl CdpClient {
    /// 连接到 Chrome CDP WebSocket
    pub async fn connect(ws_url: &str) -> Result<Self, String> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .map_err(|e| format!("CDP WebSocket 连接失败: {}", e))?;

        let (writer, mut reader) = ws_stream.split();
        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // 后台读取 WebSocket 消息
        let pending_clone = pending.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = reader.next().await {
                if let tokio_tungstenite::tungstenite::Message::Text(text) = msg {
                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(id) = data["id"].as_u64() {
                            let mut map = pending_clone.lock().await;
                            if let Some(tx) = map.remove(&id) {
                                let _ = tx.send(data);
                            }
                        }
                        // 忽略事件通知（method 字段的消息）
                    }
                }
            }
        });

        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
            next_id: AtomicU64::new(1),
            pending,
        })
    }

    /// 发送 CDP 命令并等待响应
    pub async fn send(&self, method: &str, params: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();

        {
            let mut map = self.pending.lock().await;
            map.insert(id, tx);
        }

        let msg = serde_json::json!({
            "id": id,
            "method": method,
            "params": params.unwrap_or(serde_json::json!({})),
        });

        {
            let mut writer = self.writer.lock().await;
            writer.send(tokio_tungstenite::tungstenite::Message::Text(
                serde_json::to_string(&msg).unwrap()
            )).await.map_err(|e| format!("CDP 发送失败: {}", e))?;
        }

        // 15 秒超时
        match tokio::time::timeout(std::time::Duration::from_secs(15), rx).await {
            Ok(Ok(resp)) => {
                if let Some(err) = resp.get("error") {
                    Err(format!("CDP 错误: {}", err["message"].as_str().unwrap_or("未知")))
                } else {
                    Ok(resp.get("result").cloned().unwrap_or(serde_json::json!({})))
                }
            }
            Ok(Err(_)) => Err("CDP 响应通道关闭".into()),
            Err(_) => Err("CDP 请求超时（15s）".into()),
        }
    }

    /// 导航到 URL
    pub async fn navigate(&self, url: &str) -> Result<(), String> {
        self.send("Page.enable", None).await?;
        self.send("Page.navigate", Some(serde_json::json!({"url": url}))).await?;
        // 等待页面加载（简单等待 2 秒）
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        Ok(())
    }

    /// 截图（返回 base64 PNG）
    pub async fn screenshot(&self, full_page: bool) -> Result<String, String> {
        self.send("Page.enable", None).await?;

        let mut params = serde_json::json!({
            "format": "png",
            "fromSurface": true,
            "captureBeyondViewport": true,
        });

        if full_page {
            let metrics = self.send("Page.getLayoutMetrics", None).await?;
            let content_size = metrics.get("cssContentSize")
                .or(metrics.get("contentSize"));
            if let Some(size) = content_size {
                let w = size["width"].as_f64().unwrap_or(1280.0);
                let h = size["height"].as_f64().unwrap_or(800.0);
                if w > 0.0 && h > 0.0 {
                    params["clip"] = serde_json::json!({
                        "x": 0, "y": 0, "width": w, "height": h, "scale": 1
                    });
                }
            }
        }

        let result = self.send("Page.captureScreenshot", Some(params)).await?;
        result["data"].as_str()
            .map(|s| s.to_string())
            .ok_or("截图失败：无数据".into())
    }

    /// 带 Label 标注的截图
    ///
    /// 在页面上注入 overlay 标注每个 ARIA ref 元素的位置和名称，
    /// 然后截图，最后清理 overlay。参考 OpenClaw screenshotWithLabels。
    pub async fn screenshot_with_labels(&self, nodes: &[AriaNode], max_labels: usize) -> Result<String, String> {
        // 1. 为每个有 backendDOMNodeId 的节点获取边界框
        let mut labels: Vec<serde_json::Value> = Vec::new();
        let _ = self.send("DOM.enable", None).await;

        for node in nodes.iter().take(max_labels) {
            let backend_id = match node.backend_node_id {
                Some(id) => id,
                None => continue,
            };

            // 跳过不可交互的结构节点
            let interactive = ["button", "link", "textbox", "searchbox", "combobox",
                "menuitem", "radio", "checkbox", "tab", "option", "heading"];
            if !interactive.iter().any(|r| node.role.contains(r)) && node.name.is_empty() {
                continue;
            }

            // 获取边界框
            let resolved = match self.send("DOM.resolveNode", Some(serde_json::json!({"backendNodeId": backend_id}))).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            let object_id = match resolved["object"]["objectId"].as_str() {
                Some(id) => id.to_string(),
                None => continue,
            };
            let rect_result = match self.send("Runtime.callFunctionOn", Some(serde_json::json!({
                "objectId": object_id,
                "functionDeclaration": "function() { const r = this.getBoundingClientRect(); return JSON.stringify({x:r.x,y:r.y,w:r.width,h:r.height}); }",
                "returnByValue": true,
            }))).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            let rect_str = rect_result["result"]["value"].as_str().unwrap_or("{}");
            let rect: serde_json::Value = serde_json::from_str(rect_str).unwrap_or_default();
            let x = rect["x"].as_f64().unwrap_or(0.0);
            let y = rect["y"].as_f64().unwrap_or(0.0);
            let w = rect["w"].as_f64().unwrap_or(0.0);
            let h = rect["h"].as_f64().unwrap_or(0.0);
            if w < 1.0 || h < 1.0 { continue; }

            labels.push(serde_json::json!({
                "ref": node.ref_id, "x": x, "y": y, "w": w, "h": h
            }));
        }

        // 2. 注入 overlay 到页面
        if !labels.is_empty() {
            let labels_json = serde_json::to_string(&labels).unwrap_or_default();
            let inject_js = format!(r#"
                (() => {{
                    document.querySelectorAll('[data-xianzhu-label]').forEach(el => el.remove());
                    const labels = {labels_json};
                    const root = document.createElement('div');
                    root.setAttribute('data-xianzhu-label', '1');
                    root.style.cssText = 'position:fixed;left:0;top:0;z-index:2147483647;pointer-events:none;font-family:monospace;';
                    for (const l of labels) {{
                        const box = document.createElement('div');
                        box.setAttribute('data-xianzhu-label', '1');
                        box.style.cssText = `position:absolute;left:${{l.x}}px;top:${{l.y}}px;width:${{l.w}}px;height:${{l.h}}px;border:2px solid #ffb020;box-sizing:border-box;`;
                        const tag = document.createElement('div');
                        tag.setAttribute('data-xianzhu-label', '1');
                        tag.textContent = l.ref;
                        tag.style.cssText = `position:absolute;left:${{l.x}}px;top:${{Math.max(0,l.y-18)}}px;background:#ffb020;color:#1a1a1a;font-size:11px;line-height:14px;padding:1px 4px;border-radius:3px;white-space:nowrap;`;
                        root.appendChild(box);
                        root.appendChild(tag);
                    }}
                    document.documentElement.appendChild(root);
                }})()
            "#);
            let _ = self.evaluate(&inject_js).await;
        }

        // 3. 截图
        let base64 = self.screenshot(false).await?;

        // 4. 清理 overlay
        let _ = self.evaluate("document.querySelectorAll('[data-xianzhu-label]').forEach(el => el.remove())").await;

        Ok(base64)
    }

    /// 获取 ARIA 无障碍树快照
    pub async fn aria_snapshot(&self, limit: usize) -> Result<Vec<AriaNode>, String> {
        let _ = self.send("Accessibility.enable", None).await;
        let result = self.send("Accessibility.getFullAXTree", None).await?;

        let nodes = result["nodes"].as_array()
            .ok_or("无法获取 AX 树")?;

        // 构建 ID → 节点映射
        let mut by_id: HashMap<String, &serde_json::Value> = HashMap::new();
        let mut child_refs: HashMap<String, Vec<String>> = HashMap::new();
        let mut all_children: std::collections::HashSet<String> = std::collections::HashSet::new();

        for node in nodes {
            if let Some(id) = node["nodeId"].as_str() {
                by_id.insert(id.to_string(), node);
                let children: Vec<String> = node["childIds"].as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                for c in &children {
                    all_children.insert(c.clone());
                }
                child_refs.insert(id.to_string(), children);
            }
        }

        // 找根节点（不被任何节点引用的）
        let root_id = nodes.iter()
            .find_map(|n| {
                let id = n["nodeId"].as_str()?;
                if !all_children.contains(id) { Some(id.to_string()) } else { None }
            })
            .unwrap_or_else(|| nodes.first().and_then(|n| n["nodeId"].as_str()).unwrap_or("").to_string());

        // DFS 遍历
        let mut output = Vec::new();
        let mut stack: Vec<(String, usize)> = vec![(root_id, 0)];

        while let Some((id, depth)) = stack.pop() {
            if output.len() >= limit { break; }

            if let Some(node) = by_id.get(&id) {
                let role = node["role"]["value"].as_str().unwrap_or("").to_string();
                let name = node["name"]["value"].as_str().unwrap_or("").to_string();
                let value = node["value"]["value"].as_str().map(String::from);
                let backend_node_id = node["backendDOMNodeId"].as_i64();

                // 跳过空的结构节点
                if !role.is_empty() && role != "none" && role != "generic" {
                    output.push(AriaNode {
                        ref_id: format!("ax{}", output.len() + 1),
                        role,
                        name,
                        value,
                        depth,
                        backend_node_id,
                    });
                }

                // 子节点入栈（逆序以保持遍历顺序）
                if let Some(children) = child_refs.get(&id) {
                    for c in children.iter().rev() {
                        stack.push((c.clone(), depth + 1));
                    }
                }
            }
        }

        Ok(output)
    }

    /// 执行 JavaScript
    pub async fn evaluate(&self, expression: &str) -> Result<serde_json::Value, String> {
        let _ = self.send("Runtime.enable", None).await;
        let result = self.send("Runtime.evaluate", Some(serde_json::json!({
            "expression": expression,
            "awaitPromise": true,
            "returnByValue": true,
            "userGesture": true,
        }))).await?;

        if let Some(exception) = result.get("exceptionDetails") {
            return Err(format!("JS 执行异常: {}", exception["text"].as_str().unwrap_or("未知")));
        }

        Ok(result.get("result").and_then(|r| r.get("value")).cloned().unwrap_or(serde_json::json!(null)))
    }

    /// 点击元素（通过坐标）
    pub async fn click(&self, x: f64, y: f64) -> Result<(), String> {
        self.send("Input.dispatchMouseEvent", Some(serde_json::json!({
            "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1
        }))).await?;
        self.send("Input.dispatchMouseEvent", Some(serde_json::json!({
            "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1
        }))).await?;
        Ok(())
    }

    /// 输入文本（逐字符）
    pub async fn type_text(&self, text: &str) -> Result<(), String> {
        for c in text.chars() {
            self.send("Input.dispatchKeyEvent", Some(serde_json::json!({
                "type": "keyDown", "text": c.to_string()
            }))).await?;
            self.send("Input.dispatchKeyEvent", Some(serde_json::json!({
                "type": "keyUp", "text": c.to_string()
            }))).await?;
        }
        Ok(())
    }

    // ─── Ref 定位系统 ─────────────────────────────────────────
    // 通过 ARIA snapshot 的 ref（如 ax15）定位元素，获取坐标或执行操作。
    // 流程：snapshot → 找到 backendDOMNodeId → DOM.resolveNode → 获取 objectId →
    //       Runtime.callFunctionOn(getBoundingClientRect) → 坐标

    /// 通过 ARIA ref 获取元素的屏幕坐标（中心点）
    pub async fn resolve_ref_coordinates(&self, nodes: &[AriaNode], ref_id: &str) -> Result<(f64, f64), String> {
        let node = nodes.iter().find(|n| n.ref_id == ref_id)
            .ok_or(format!("未找到 ref: {}。请先执行 snapshot 获取最新的 ref 列表。", ref_id))?;

        let backend_node_id = node.backend_node_id
            .ok_or(format!("ref {} 没有关联的 DOM 节点（role={}）", ref_id, node.role))?;

        // DOM.resolveNode → 获取 JS objectId
        let resolved = self.send("DOM.resolveNode", Some(serde_json::json!({
            "backendNodeId": backend_node_id
        }))).await?;

        let object_id = resolved["object"]["objectId"].as_str()
            .ok_or("无法获取元素的 objectId")?;

        // 调用 getBoundingClientRect 获取位置
        let rect_result = self.send("Runtime.callFunctionOn", Some(serde_json::json!({
            "objectId": object_id,
            "functionDeclaration": "function() { const r = this.getBoundingClientRect(); return JSON.stringify({x: r.x, y: r.y, w: r.width, h: r.height}); }",
            "returnByValue": true,
        }))).await?;

        let rect_str = rect_result["result"]["value"].as_str().unwrap_or("{}");
        let rect: serde_json::Value = serde_json::from_str(rect_str).unwrap_or_default();

        let x = rect["x"].as_f64().unwrap_or(0.0);
        let y = rect["y"].as_f64().unwrap_or(0.0);
        let w = rect["w"].as_f64().unwrap_or(0.0);
        let h = rect["h"].as_f64().unwrap_or(0.0);

        if w == 0.0 && h == 0.0 {
            return Err(format!("ref {} 元素不可见或大小为 0", ref_id));
        }

        // 返回中心点坐标
        Ok((x + w / 2.0, y + h / 2.0))
    }

    /// 通过 ref 点击元素
    pub async fn click_ref(&self, nodes: &[AriaNode], ref_id: &str) -> Result<(), String> {
        let (x, y) = self.resolve_ref_coordinates(nodes, ref_id).await?;
        self.click(x, y).await
    }

    /// 通过 ref 填写表单字段
    pub async fn fill_ref(&self, nodes: &[AriaNode], ref_id: &str, value: &str) -> Result<(), String> {
        let node = nodes.iter().find(|n| n.ref_id == ref_id)
            .ok_or(format!("未找到 ref: {}", ref_id))?;

        let backend_node_id = node.backend_node_id
            .ok_or(format!("ref {} 没有关联的 DOM 节点", ref_id))?;

        let resolved = self.send("DOM.resolveNode", Some(serde_json::json!({
            "backendNodeId": backend_node_id
        }))).await?;

        let object_id = resolved["object"]["objectId"].as_str()
            .ok_or("无法获取元素的 objectId")?;

        // 先 focus，再设值，再触发事件
        let escaped_value = value.replace('\\', "\\\\").replace('\'', "\\'").replace('\n', "\\n");
        self.send("Runtime.callFunctionOn", Some(serde_json::json!({
            "objectId": object_id,
            "functionDeclaration": format!(
                "function() {{ this.focus(); this.value = '{}'; this.dispatchEvent(new Event('input', {{bubbles:true}})); this.dispatchEvent(new Event('change', {{bubbles:true}})); }}",
                escaped_value
            ),
            "returnByValue": true,
        }))).await?;

        Ok(())
    }

    /// 通过 ref 设置 checkbox/radio 选中状态
    pub async fn check_ref(&self, nodes: &[AriaNode], ref_id: &str, checked: bool) -> Result<(), String> {
        let node = nodes.iter().find(|n| n.ref_id == ref_id)
            .ok_or(format!("未找到 ref: {}", ref_id))?;

        let backend_node_id = node.backend_node_id
            .ok_or(format!("ref {} 没有关联的 DOM 节点", ref_id))?;

        let resolved = self.send("DOM.resolveNode", Some(serde_json::json!({
            "backendNodeId": backend_node_id
        }))).await?;

        let object_id = resolved["object"]["objectId"].as_str()
            .ok_or("无法获取元素的 objectId")?;

        self.send("Runtime.callFunctionOn", Some(serde_json::json!({
            "objectId": object_id,
            "functionDeclaration": format!(
                "function() {{ this.checked = {}; this.dispatchEvent(new Event('change', {{bubbles:true}})); }}",
                checked
            ),
            "returnByValue": true,
        }))).await?;

        Ok(())
    }

    /// 通过 ref 悬停元素
    pub async fn hover_ref(&self, nodes: &[AriaNode], ref_id: &str) -> Result<(), String> {
        let (x, y) = self.resolve_ref_coordinates(nodes, ref_id).await?;
        self.hover(x, y).await
    }

    /// 通过 ref 上传文件
    pub async fn upload_ref(&self, nodes: &[AriaNode], ref_id: &str, file_paths: &[String]) -> Result<(), String> {
        let node = nodes.iter().find(|n| n.ref_id == ref_id)
            .ok_or(format!("未找到 ref: {}", ref_id))?;

        let backend_node_id = node.backend_node_id
            .ok_or(format!("ref {} 没有关联的 DOM 节点", ref_id))?;

        // DOM.setFileInputFiles 直接用 backendNodeId
        self.send("DOM.enable", None).await?;
        self.send("DOM.setFileInputFiles", Some(serde_json::json!({
            "files": file_paths,
            "backendNodeId": backend_node_id,
        }))).await?;

        Ok(())
    }

    /// 悬停（hover）
    pub async fn hover(&self, x: f64, y: f64) -> Result<(), String> {
        self.send("Input.dispatchMouseEvent", Some(serde_json::json!({
            "type": "mouseMoved", "x": x, "y": y
        }))).await?;
        Ok(())
    }

    /// 双击
    pub async fn double_click(&self, x: f64, y: f64) -> Result<(), String> {
        self.send("Input.dispatchMouseEvent", Some(serde_json::json!({
            "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 2
        }))).await?;
        self.send("Input.dispatchMouseEvent", Some(serde_json::json!({
            "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 2
        }))).await?;
        Ok(())
    }

    /// 拖拽（从 A 到 B）
    pub async fn drag(&self, from_x: f64, from_y: f64, to_x: f64, to_y: f64) -> Result<(), String> {
        // mouseDown at from
        self.send("Input.dispatchMouseEvent", Some(serde_json::json!({
            "type": "mousePressed", "x": from_x, "y": from_y, "button": "left", "clickCount": 1
        }))).await?;
        // mouseMoved to target（分几步模拟平滑拖拽）
        let steps = 10;
        for i in 1..=steps {
            let ratio = i as f64 / steps as f64;
            let x = from_x + (to_x - from_x) * ratio;
            let y = from_y + (to_y - from_y) * ratio;
            self.send("Input.dispatchMouseEvent", Some(serde_json::json!({
                "type": "mouseMoved", "x": x, "y": y, "button": "left"
            }))).await?;
        }
        // mouseUp at to
        self.send("Input.dispatchMouseEvent", Some(serde_json::json!({
            "type": "mouseReleased", "x": to_x, "y": to_y, "button": "left", "clickCount": 1
        }))).await?;
        Ok(())
    }

    /// 按键（Enter, Tab, Escape, Backspace 等）
    pub async fn press_key(&self, key: &str) -> Result<(), String> {
        // 常用键映射
        let (key_code, code) = match key.to_lowercase().as_str() {
            "enter" | "return" => ("Enter", "Enter"),
            "tab" => ("Tab", "Tab"),
            "escape" | "esc" => ("Escape", "Escape"),
            "backspace" => ("Backspace", "Backspace"),
            "delete" => ("Delete", "Delete"),
            "arrowup" | "up" => ("ArrowUp", "ArrowUp"),
            "arrowdown" | "down" => ("ArrowDown", "ArrowDown"),
            "arrowleft" | "left" => ("ArrowLeft", "ArrowLeft"),
            "arrowright" | "right" => ("ArrowRight", "ArrowRight"),
            "space" => (" ", "Space"),
            "home" => ("Home", "Home"),
            "end" => ("End", "End"),
            "pageup" => ("PageUp", "PageUp"),
            "pagedown" => ("PageDown", "PageDown"),
            _ => (key, key),
        };

        self.send("Input.dispatchKeyEvent", Some(serde_json::json!({
            "type": "rawKeyDown", "key": key_code, "code": code, "windowsVirtualKeyCode": 13
        }))).await?;
        self.send("Input.dispatchKeyEvent", Some(serde_json::json!({
            "type": "keyUp", "key": key_code, "code": code
        }))).await?;
        Ok(())
    }

    /// 通过 CSS 选择器填写表单字段
    pub async fn fill_form(&self, fields: &[(String, String)]) -> Result<String, String> {
        let mut filled = 0;
        for (selector, value) in fields {
            // 用 JS focus + 设值
            let js = format!(
                r#"(() => {{
                    const el = document.querySelector('{}');
                    if (!el) return 'not_found';
                    el.focus();
                    el.value = '{}';
                    el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                    return 'ok';
                }})()"#,
                selector.replace('\'', "\\'").replace('\\', "\\\\"),
                value.replace('\'', "\\'").replace('\\', "\\\\"),
            );
            let result = self.evaluate(&js).await?;
            if result.as_str() == Some("ok") {
                filled += 1;
            } else {
                log::warn!("CDP fill_form: 选择器 {} 未找到元素", selector);
            }
        }
        Ok(format!("已填写 {}/{} 个字段", filled, fields.len()))
    }

    /// 处理对话框（alert/confirm/prompt）
    pub async fn handle_dialog(&self, accept: bool, prompt_text: Option<&str>) -> Result<(), String> {
        let _ = self.send("Page.enable", None).await;
        self.send("Page.handleJavaScriptDialog", Some(serde_json::json!({
            "accept": accept,
            "promptText": prompt_text.unwrap_or(""),
        }))).await?;
        Ok(())
    }

    /// 设置文件输入（通过 DOM.setFileInputFiles）
    pub async fn set_file_input(&self, selector: &str, file_paths: &[String]) -> Result<(), String> {
        let _ = self.send("DOM.enable", None).await;
        // 获取 document root
        let doc = self.send("DOM.getDocument", None).await?;
        let root_id = doc["root"]["nodeId"].as_i64().ok_or("无法获取 DOM root")?;

        // querySelector
        let node = self.send("DOM.querySelector", Some(serde_json::json!({
            "nodeId": root_id, "selector": selector
        }))).await?;
        let node_id = node["nodeId"].as_i64().ok_or(format!("选择器 {} 未匹配元素", selector))?;

        // setFileInputFiles
        self.send("DOM.setFileInputFiles", Some(serde_json::json!({
            "files": file_paths, "nodeId": node_id
        }))).await?;

        Ok(())
    }

    /// 调整视口大小
    pub async fn resize(&self, width: u32, height: u32) -> Result<(), String> {
        self.send("Emulation.setDeviceMetricsOverride", Some(serde_json::json!({
            "width": width, "height": height,
            "deviceScaleFactor": 1, "mobile": false,
        }))).await?;
        Ok(())
    }

    /// 滚动页面
    pub async fn scroll(&self, x: f64, y: f64, delta_x: f64, delta_y: f64) -> Result<(), String> {
        self.send("Input.dispatchMouseEvent", Some(serde_json::json!({
            "type": "mouseWheel", "x": x, "y": y,
            "deltaX": delta_x, "deltaY": delta_y,
        }))).await?;
        Ok(())
    }

    /// 获取页面 URL 和标题
    pub async fn get_page_info(&self) -> Result<(String, String), String> {
        let result = self.evaluate("JSON.stringify({url: location.href, title: document.title})").await?;
        let info_str = result.as_str().unwrap_or("{}");
        let info: serde_json::Value = serde_json::from_str(info_str).unwrap_or_default();
        Ok((
            info["url"].as_str().unwrap_or("").to_string(),
            info["title"].as_str().unwrap_or("").to_string(),
        ))
    }

    /// 等待选择器出现
    pub async fn wait_for_selector(&self, selector: &str, timeout_ms: u64) -> Result<(), String> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);
        loop {
            let js = format!(
                "document.querySelector('{}') !== null",
                selector.replace('\'', "\\'")
            );
            let found = self.evaluate(&js).await?;
            if found.as_bool() == Some(true) {
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err(format!("等待选择器 {} 超时（{}ms）", selector, timeout_ms));
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
}

// ─── Chrome 进程管理 ─────────────────────────────────────────

/// 管理的 Chrome 实例
pub struct ManagedChrome {
    pub pid: u32,
    pub cdp_port: u16,
    pub ws_url: String,
    process: tokio::process::Child,
}

impl ManagedChrome {
    /// 停止 Chrome
    pub async fn stop(&mut self) {
        let _ = self.process.kill().await;
        log::info!("CDP Chrome 已停止 (pid={})", self.pid);
    }
}

impl Drop for ManagedChrome {
    fn drop(&mut self) {
        // 尝试终止进程
        let _ = self.process.start_kill();
    }
}

/// 启动受管 Chrome 实例（隔离 profile）
pub async fn launch_chrome(
    executable: &str,
    cdp_port: u16,
    headless: bool,
) -> Result<ManagedChrome, String> {
    // 创建隔离的 user data dir
    let user_data_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("com.xianzhu.app/chrome-profile");
    let _ = std::fs::create_dir_all(&user_data_dir);

    let mut args = vec![
        format!("--remote-debugging-port={}", cdp_port),
        format!("--user-data-dir={}", user_data_dir.display()),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        "--disable-sync".to_string(),
        "--disable-background-networking".to_string(),
        "--disable-component-update".to_string(),
        "--disable-features=Translate,MediaRouter".to_string(),
        "--disable-session-crashed-bubble".to_string(),
        "--password-store=basic".to_string(),
    ];

    if headless {
        args.push("--headless=new".to_string());
        args.push("--disable-gpu".to_string());
    }

    #[cfg(target_os = "linux")]
    args.push("--disable-dev-shm-usage".to_string());

    log::info!("CDP 启动 Chrome: {} port={} headless={}", executable, cdp_port, headless);

    let process = tokio::process::Command::new(executable)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 Chrome 失败: {}", e))?;

    let pid = process.id().unwrap_or(0);

    // 轮询等待 CDP 就绪（最多 25 秒）
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build().unwrap_or_else(|_| reqwest::Client::new());

    let version_url = format!("http://127.0.0.1:{}/json/version", cdp_port);
    let mut ws_url = String::new();

    for _ in 0..250 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if let Ok(resp) = client.get(&version_url).send().await {
            if let Ok(data) = resp.json::<serde_json::Value>().await {
                if let Some(url) = data["webSocketDebuggerUrl"].as_str() {
                    ws_url = url.to_string();
                    break;
                }
            }
        }
    }

    if ws_url.is_empty() {
        return Err("Chrome CDP 未就绪（25s 超时）".into());
    }

    log::info!("CDP Chrome 已就绪: pid={} ws={}", pid, ws_url);

    Ok(ManagedChrome {
        pid,
        cdp_port,
        ws_url,
        process,
    })
}

/// 获取 Chrome 打开的标签页列表
pub async fn list_tabs(cdp_port: u16) -> Result<Vec<TabInfo>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build().map_err(|e| e.to_string())?;

    let resp = client.get(format!("http://127.0.0.1:{}/json/list", cdp_port))
        .send().await
        .map_err(|e| format!("获取 Tab 列表失败: {}", e))?;

    let tabs: Vec<TabInfo> = resp.json().await
        .map_err(|e| format!("解析 Tab 列表失败: {}", e))?;

    Ok(tabs.into_iter().filter(|t| t.tab_type == "page").collect())
}

// ─── 用户 Chrome 连接（existing-session 模式）────────────────

/// 检测用户 Chrome 是否开启了 remote debugging
///
/// 扫描常用端口（9222-9229），返回可连接的端口号
pub async fn detect_user_chrome_debug_port() -> Option<u16> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build().ok()?;

    for port in [9222, 9223, 9224, 9225, 9226, 9227, 9228, 9229] {
        let url = format!("http://127.0.0.1:{}/json/version", port);
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                log::info!("检测到用户 Chrome debug 端口: {}", port);
                return Some(port);
            }
        }
    }
    None
}

/// 连接用户已运行的 Chrome（existing-session 模式）
///
/// 不启动新 Chrome，直接连接已开启 remote-debugging 的用户浏览器。
/// 这样可以访问用户已登录的网站（带 Cookie/登录态）。
pub async fn connect_user_chrome() -> Result<(u16, String), String> {
    let port = detect_user_chrome_debug_port().await
        .ok_or_else(|| {
            let hint = if cfg!(target_os = "macos") {
                "请用以下命令启动 Chrome:\n  /Applications/Google\\ Chrome.app/Contents/MacOS/Google\\ Chrome --remote-debugging-port=9222\n\n或 Brave:\n  /Applications/Brave\\ Browser.app/Contents/MacOS/Brave\\ Browser --remote-debugging-port=9222"
            } else if cfg!(target_os = "linux") {
                "请用以下命令启动 Chrome:\n  google-chrome --remote-debugging-port=9222\n\n或 Brave:\n  brave-browser --remote-debugging-port=9222"
            } else {
                "请用以下命令启动 Chrome:\n  chrome.exe --remote-debugging-port=9222"
            };
            format!("未检测到用户 Chrome 的 remote-debugging 端口。\n\n{}", hint)
        })?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build().map_err(|e| e.to_string())?;

    let resp = client.get(format!("http://127.0.0.1:{}/json/version", port))
        .send().await.map_err(|e| format!("连接失败: {}", e))?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析失败: {}", e))?;

    let ws_url = data["webSocketDebuggerUrl"].as_str()
        .ok_or("Chrome 未返回 WebSocket URL")?
        .to_string();

    let browser = data["Browser"].as_str().unwrap_or("Unknown");
    log::info!("已连接用户 Chrome: {} (port={}, ws={})", browser, port, ws_url);

    Ok((port, ws_url))
}

/// 获取用户 Chrome 的默认 profile 目录
pub fn get_user_chrome_profile_dir() -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;

    #[cfg(target_os = "macos")]
    {
        let candidates = [
            home.join("Library/Application Support/Google/Chrome/Default"),
            home.join("Library/Application Support/BraveSoftware/Brave-Browser/Default"),
            home.join("Library/Application Support/Microsoft Edge/Default"),
        ];
        for c in &candidates {
            if c.exists() { return Some(c.clone()); }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let candidates = [
            home.join(".config/google-chrome/Default"),
            home.join(".config/BraveSoftware/Brave-Browser/Default"),
            home.join(".config/microsoft-edge/Default"),
            home.join(".config/chromium/Default"),
        ];
        for c in &candidates {
            if c.exists() { return Some(c.clone()); }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app) = dirs::data_local_dir() {
            let candidates = [
                local_app.join("Google/Chrome/User Data/Default"),
                local_app.join("BraveSoftware/Brave-Browser/User Data/Default"),
                local_app.join("Microsoft/Edge/User Data/Default"),
            ];
            for c in &candidates {
                if c.exists() { return Some(c.clone()); }
            }
        }
    }

    None
}

/// 格式化 ARIA 快照为可读文本
pub fn format_aria_snapshot(nodes: &[AriaNode]) -> String {
    let mut lines = Vec::new();
    for node in nodes {
        let indent = "  ".repeat(node.depth);
        let mut line = format!("{}- {}", indent, node.role);
        if !node.name.is_empty() {
            line.push_str(&format!(" \"{}\"", node.name));
        }
        line.push_str(&format!(" [ref={}]", node.ref_id));
        if let Some(ref v) = node.value {
            if !v.is_empty() {
                line.push_str(&format!(" value=\"{}\"", v));
            }
        }
        lines.push(line);
    }
    lines.join("\n")
}
