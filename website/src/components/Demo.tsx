import { motion, useInView } from 'framer-motion';
import { useRef, useState, useEffect } from 'react';

/** 模拟的聊天消息 */
interface Message {
  role: 'user' | 'assistant' | 'tool';
  content: string;
  toolName?: string;
}

const conversation: Message[] = [
  { role: 'user', content: '帮我搜索 Rust 异步运行时的对比分析' },
  { role: 'tool', content: '正在搜索...找到 12 条相关结果', toolName: 'web_search' },
  {
    role: 'assistant',
    content:
      '根据搜索结果，Rust 主流异步运行时有三个：\n\n1. **Tokio** — 最成熟，生态最大，适合服务端\n2. **async-std** — API 接近标准库，学习曲线低\n3. **smol** — 极简设计，适合嵌入式场景\n\n对于桌面应用，推荐 Tokio，因为 Tauri 已内置依赖。',
  },
];

/** 打字机效果 Hook */
function useTypewriter(text: string, speed: number, startTyping: boolean) {
  const [displayed, setDisplayed] = useState('');
  const [done, setDone] = useState(false);

  useEffect(() => {
    if (!startTyping) return;
    setDisplayed('');
    setDone(false);
    let i = 0;
    const interval = setInterval(() => {
      i++;
      setDisplayed(text.slice(0, i));
      if (i >= text.length) {
        clearInterval(interval);
        setDone(true);
      }
    }, speed);
    return () => clearInterval(interval);
  }, [text, speed, startTyping]);

  return { displayed, done };
}

function ChatBubble({ msg, index, startTyping }: { msg: Message; index: number; startTyping: boolean }) {
  const isLast = index === conversation.length - 1;
  const { displayed, done } = useTypewriter(
    msg.content,
    isLast ? 18 : 10,
    startTyping
  );

  if (msg.role === 'user') {
    return (
      <motion.div
        initial={{ opacity: 0, x: 20 }}
        animate={{ opacity: 1, x: 0 }}
        transition={{ duration: 0.4 }}
        className="self-end max-w-sm"
      >
        <div className="bg-indigo-500/10 border border-indigo-500/20 rounded-2xl rounded-tr-md px-4 py-3 text-sm text-white/80">
          {msg.content}
        </div>
      </motion.div>
    );
  }

  if (msg.role === 'tool') {
    return (
      <motion.div
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.3 }}
        className="self-start"
      >
        <div className="inline-flex items-center gap-2 px-3 py-1.5 rounded-lg bg-amber-500/5 border border-amber-500/10 text-xs font-mono">
          <span className="text-amber-400">[{msg.toolName}]</span>
          <span className="text-white/40">{msg.content}</span>
        </div>
      </motion.div>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="self-start max-w-md"
    >
      <div className="bg-white/[0.03] border border-white/[0.06] rounded-2xl rounded-tl-md px-4 py-3 text-sm text-white/60 leading-relaxed whitespace-pre-line">
        {startTyping ? displayed : ''}
        {startTyping && !done && <span className="typing-cursor" />}
      </div>
    </motion.div>
  );
}

export default function Demo() {
  const ref = useRef<HTMLDivElement>(null);
  const isInView = useInView(ref, { once: true, margin: '-100px' });
  const [visibleCount, setVisibleCount] = useState(0);

  useEffect(() => {
    if (!isInView) return;
    // 逐步显示消息
    const delays = [0, 800, 1600];
    const timers = delays.map((delay, i) =>
      setTimeout(() => setVisibleCount(i + 1), delay)
    );
    return () => timers.forEach(clearTimeout);
  }, [isInView]);

  return (
    <section className="relative py-32 px-4">
      <div className="max-w-4xl mx-auto">
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.6 }}
          className="text-center mb-16"
        >
          <h2 className="text-3xl md:text-5xl font-bold mb-4">
            <span className="gradient-text">智能对话</span>，不止于此
          </h2>
          <p className="text-white/40 text-lg">
            工具调用、多步推理、上下文记忆，一切自然发生
          </p>
        </motion.div>

        <div ref={ref}>
          {/* 聊天窗口 Mock */}
          <div className="rounded-2xl border border-white/[0.08] bg-[#12121a]/60 backdrop-blur-xl overflow-hidden">
            {/* 顶栏 */}
            <div className="flex items-center gap-2 px-5 py-3 border-b border-white/[0.06]">
              <div className="w-3 h-3 rounded-full bg-[#ff5f57]" />
              <div className="w-3 h-3 rounded-full bg-[#febc2e]" />
              <div className="w-3 h-3 rounded-full bg-[#28c840]" />
              <span className="ml-3 text-xs text-white/30">Claude Sonnet 4 - 对话</span>
            </div>
            {/* 聊天内容 */}
            <div className="p-6 flex flex-col gap-4 min-h-[320px]">
              {conversation.map((msg, i) =>
                i < visibleCount ? (
                  <ChatBubble
                    key={i}
                    msg={msg}
                    index={i}
                    startTyping={true}
                  />
                ) : null
              )}
            </div>
            {/* 输入框 */}
            <div className="px-5 pb-5">
              <div className="flex items-center gap-3 rounded-xl bg-white/[0.03] border border-white/[0.06] px-4 py-3">
                <span className="text-white/20 text-sm flex-1">输入消息...</span>
                <div className="w-8 h-8 rounded-lg bg-amber-500/10 flex items-center justify-center">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#f59e0b" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <line x1="22" y1="2" x2="11" y2="13" />
                    <polygon points="22 2 15 22 11 13 2 9 22 2" />
                  </svg>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
