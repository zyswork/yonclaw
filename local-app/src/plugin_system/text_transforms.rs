//! 插件文本 Transform Hooks
//!
//! 参照 OpenClaw #202f80792e：允许插件在用户输入→LLM 前 / LLM 输出→用户前
//! 插入文本变换。典型用途：
//! - 术语替换（将"甲方"自动替换为公司全名）
//! - 敏感词过滤
//! - 模板展开（@today → 2026-04-16）
//! - 缩写扩展

use std::collections::HashMap;

/// Transform 作用方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformStage {
    /// 用户输入 → LLM
    PreLlm,
    /// LLM 输出 → 用户
    PostLlm,
}

/// 单个 Transform 规则
#[derive(Debug, Clone)]
pub struct TextTransform {
    pub id: String,
    pub name: String,
    pub pattern: String,      // 正则
    pub replacement: String,  // 替换串（支持 $1, $2）
    pub stage: TransformStage,
    pub enabled: bool,
}

/// Transform 注册表
///
/// 接入点：`Orchestrator.text_transforms`（RwLock 保护）
/// - PreLlm：在 `send_message_stream` 入口生效（用户输入 → LLM 前）
/// - PostLlm：尚未接入流式输出（流式 token 无法一次性变换）
#[derive(Default)]
pub struct TransformRegistry {
    transforms: Vec<TextTransform>,
    regex_cache: HashMap<String, regex::Regex>,
}

impl TransformRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个 transform
    pub fn register(&mut self, t: TextTransform) -> Result<(), String> {
        // 预编译 regex
        regex::Regex::new(&t.pattern)
            .map_err(|e| format!("正则编译失败: {}", e))
            .map(|re| {
                self.regex_cache.insert(t.id.clone(), re);
                // 替换或追加
                if let Some(idx) = self.transforms.iter().position(|x| x.id == t.id) {
                    self.transforms[idx] = t;
                } else {
                    self.transforms.push(t);
                }
            })
    }

    pub fn remove(&mut self, id: &str) {
        self.transforms.retain(|t| t.id != id);
        self.regex_cache.remove(id);
    }

    pub fn list(&self) -> &[TextTransform] {
        &self.transforms
    }

    /// 应用所有启用的、指定 stage 的 transforms
    pub fn apply(&self, text: &str, stage: TransformStage) -> String {
        let mut result = text.to_string();
        for t in self.transforms.iter().filter(|t| t.enabled && t.stage == stage) {
            if let Some(re) = self.regex_cache.get(&t.id) {
                result = re.replace_all(&result, t.replacement.as_str()).to_string();
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_replace() {
        let mut reg = TransformRegistry::new();
        reg.register(TextTransform {
            id: "t1".into(), name: "年份".into(),
            pattern: r"今年".into(), replacement: "2026 年".into(),
            stage: TransformStage::PreLlm, enabled: true,
        }).unwrap();
        let out = reg.apply("今年是牛年", TransformStage::PreLlm);
        assert_eq!(out, "2026 年是牛年");
    }

    #[test]
    fn test_disabled_no_effect() {
        let mut reg = TransformRegistry::new();
        reg.register(TextTransform {
            id: "t1".into(), name: "test".into(),
            pattern: r"x".into(), replacement: "y".into(),
            stage: TransformStage::PreLlm, enabled: false,
        }).unwrap();
        assert_eq!(reg.apply("xx", TransformStage::PreLlm), "xx");
    }

    #[test]
    fn test_stage_isolation() {
        let mut reg = TransformRegistry::new();
        reg.register(TextTransform {
            id: "pre".into(), name: "".into(),
            pattern: "a".into(), replacement: "b".into(),
            stage: TransformStage::PreLlm, enabled: true,
        }).unwrap();
        reg.register(TextTransform {
            id: "post".into(), name: "".into(),
            pattern: "b".into(), replacement: "c".into(),
            stage: TransformStage::PostLlm, enabled: true,
        }).unwrap();
        assert_eq!(reg.apply("a", TransformStage::PreLlm), "b");
        assert_eq!(reg.apply("b", TransformStage::PostLlm), "c");
    }
}
