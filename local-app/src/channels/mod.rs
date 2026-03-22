//! 频道系统 — Telegram/飞书/钉钉等外部渠道
//!
//! Telegram 轮询在桌面端本地执行（不走云端中转），延迟最低。
//! 桌面端离线时，云端自动接管。

pub mod telegram;
pub mod feishu;
pub mod weixin;
