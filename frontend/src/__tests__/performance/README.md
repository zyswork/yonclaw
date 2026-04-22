# Performance Benchmarks

## 目标

这些基准用于防止性能退化。每次重大改动后应跑一遍对比。

## 覆盖范围

1. **Token counter** — `TokenCounter::count` 在 10k 字符上的速度
2. **Memory search** — 1000 条记忆 FTS5 + RRF 混合搜索延迟
3. **Context compression** — 100 条消息的 compact_session 端到端
4. **Session message cache** — 10 并发读写的延迟

## 运行

### Rust 基准（后端）
```bash
cd local-app
cargo bench --bench my_bench
```

### 前端（vitest bench）
```bash
cd frontend
npx vitest bench
```

## 基线（2026-04-17）

- `estimate_tokens(10k_chars)` ~ < 5ms
- memory `recall` on 1k entries ~ < 100ms
- compact_session（100 msg）~ < 5s（含 LLM 调用）
- session_msg_cache 10 并发 ~ < 10ms

## 待补

- [ ] Rust `cargo bench` 骨架（criterion）
- [ ] 大记忆库（10k+）的 FTS 压测
- [ ] 长会话（1000+ 消息）的 token_counter 回归
- [ ] 向量检索并发测试
