import { motion } from 'framer-motion';
import { useEffect, useState, useMemo } from 'react';

/** 检测操作系统平台 */
function detectPlatform(): 'macos' | 'windows' | 'linux' {
  if (typeof navigator === 'undefined') return 'macos';
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('mac')) return 'macos';
  if (ua.includes('win')) return 'windows';
  return 'linux';
}

/** macOS 图标 */
const AppleIcon = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
    <path d="M18.71 19.5c-.83 1.24-1.71 2.45-3.05 2.47-1.34.03-1.77-.79-3.29-.79-1.53 0-2 .77-3.27.82-1.31.05-2.3-1.32-3.14-2.53C4.25 17 2.94 12.45 4.7 9.39c.87-1.52 2.43-2.48 4.12-2.51 1.28-.02 2.5.87 3.29.87.78 0 2.26-1.07 3.8-.91.65.03 2.47.26 3.64 1.98-.09.06-2.17 1.28-2.15 3.81.03 3.02 2.65 4.03 2.68 4.04-.03.07-.42 1.44-1.38 2.83M13 3.5c.73-.83 1.94-1.46 2.94-1.5.13 1.17-.34 2.35-1.04 3.19-.69.85-1.83 1.51-2.95 1.42-.15-1.15.41-2.35 1.05-3.11z"/>
  </svg>
);

/** Windows 图标 */
const WindowsIcon = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
    <path d="M3 12V6.75l6-1.32v6.48L3 12zm6.2.13l.01 6.42-6.2-1.01V12.5l6.19-.37zM10.2 5.25l9.8-1.5v8.11l-9.8.1V5.25zm9.79 6.61l.01 8.39-9.8-1.5V12l9.79-.14z"/>
  </svg>
);

/** Linux 图标 */
const LinuxIcon = () => (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
    <path d="M12.504 0c-.155 0-.315.008-.48.021-4.226.333-3.105 4.807-3.17 6.298-.076 1.092-.3 1.953-1.05 3.02-.885 1.051-2.127 2.75-2.716 4.521-.278.832-.41 1.684-.287 2.489.117.811.475 1.541 1.139 2.021.603.437 1.376.624 2.271.559 1.418-.103 2.143-.628 3.228-.843.385-.076.788-.12 1.225-.113.436-.007.84.037 1.225.113 1.085.215 1.81.74 3.228.843.895.065 1.668-.122 2.27-.559.665-.48 1.023-1.21 1.14-2.021.123-.805-.01-1.657-.288-2.489-.589-1.771-1.831-3.47-2.715-4.521-.75-1.067-.975-1.928-1.051-3.02-.065-1.491 1.056-5.965-3.17-6.298A4.003 4.003 0 0 0 12.504 0z"/>
  </svg>
);

/** 浮动星星 */
function Stars() {
  const stars = useMemo(
    () =>
      Array.from({ length: 30 }, (_, i) => ({
        id: i,
        left: `${Math.random() * 100}%`,
        animationDuration: `${8 + Math.random() * 12}s`,
        animationDelay: `${Math.random() * 10}s`,
        size: `${1 + Math.random() * 2}px`,
        bottom: `${-10 - Math.random() * 20}%`,
      })),
    []
  );

  return (
    <div className="stars-container">
      {stars.map((s) => (
        <div
          key={s.id}
          className="star"
          style={{
            left: s.left,
            bottom: s.bottom,
            width: s.size,
            height: s.size,
            animationDuration: s.animationDuration,
            animationDelay: s.animationDelay,
          }}
        />
      ))}
    </div>
  );
}

/** 产品截图 Mock */
function ProductMockup() {
  return (
    <motion.div
      initial={{ opacity: 0, y: 60 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.8, delay: 0.6, ease: [0.22, 1, 0.36, 1] }}
      className="relative mx-auto mt-16 max-w-4xl px-4"
    >
      {/* 背景发光 */}
      <div
        className="absolute inset-0 -z-10 blur-[80px] opacity-30"
        style={{
          background:
            'radial-gradient(ellipse at center, rgba(245,158,11,0.3), rgba(99,102,241,0.2), transparent 70%)',
        }}
      />
      {/* 窗口容器 */}
      <div className="rounded-2xl border border-white/[0.08] bg-[#12121a]/80 backdrop-blur-xl overflow-hidden shadow-2xl">
        {/* 标题栏 */}
        <div className="flex items-center gap-2 px-4 py-3 border-b border-white/[0.06]">
          <div className="w-3 h-3 rounded-full bg-[#ff5f57]" />
          <div className="w-3 h-3 rounded-full bg-[#febc2e]" />
          <div className="w-3 h-3 rounded-full bg-[#28c840]" />
          <span className="ml-3 text-xs text-white/30 font-medium tracking-wide">
            XianZhu
          </span>
        </div>
        {/* 内容区 */}
        <div className="p-6 min-h-[280px] flex">
          {/* 侧栏 */}
          <div className="w-48 border-r border-white/[0.06] pr-4 hidden md:block">
            <div className="text-xs text-white/30 uppercase tracking-wider mb-3">
              Agents
            </div>
            {['GPT-4o', 'Claude Sonnet', 'Gemini Pro', 'Kimi'].map(
              (name, i) => (
                <div
                  key={name}
                  className={`px-3 py-2 rounded-lg text-sm mb-1 ${
                    i === 1
                      ? 'bg-amber-500/10 text-amber-400 border border-amber-500/20'
                      : 'text-white/40 hover:text-white/60'
                  }`}
                >
                  {name}
                </div>
              )
            )}
            <div className="text-xs text-white/30 uppercase tracking-wider mb-3 mt-6">
              Skills
            </div>
            {['web_search', 'code_exec', 'file_read'].map((s) => (
              <div key={s} className="px-3 py-1.5 rounded text-xs text-white/30 mb-1 font-mono">
                {s}
              </div>
            ))}
          </div>
          {/* 聊天区 */}
          <div className="flex-1 pl-0 md:pl-6 flex flex-col gap-3">
            <div className="self-end max-w-xs bg-indigo-500/10 border border-indigo-500/20 rounded-2xl rounded-tr-md px-4 py-2.5 text-sm text-white/80">
              帮我分析一下这个项目的架构
            </div>
            <div className="self-start max-w-sm bg-white/[0.03] border border-white/[0.06] rounded-2xl rounded-tl-md px-4 py-2.5 text-sm text-white/60">
              <span className="text-amber-400 text-xs font-mono block mb-1.5">
                [web_search] 搜索项目结构...
              </span>
              我已经分析了项目结构。这是一个基于 Tauri 的桌面应用，采用 Rust 后端 + React 前端...
            </div>
          </div>
        </div>
      </div>
    </motion.div>
  );
}

export default function Hero() {
  const [platform, setPlatform] = useState<'macos' | 'windows' | 'linux'>('macos');

  useEffect(() => {
    setPlatform(detectPlatform());
  }, []);

  const PlatformIcon =
    platform === 'macos' ? AppleIcon : platform === 'windows' ? WindowsIcon : LinuxIcon;

  const platformLabel =
    platform === 'macos' ? 'macOS' : platform === 'windows' ? 'Windows' : 'Linux';

  return (
    <section className="relative min-h-screen flex flex-col justify-center overflow-hidden">
      <Stars />

      {/* 顶部渐变光晕 */}
      <div
        className="absolute top-[-20%] left-1/2 -translate-x-1/2 w-[800px] h-[600px] pointer-events-none opacity-20"
        style={{
          background:
            'radial-gradient(ellipse, rgba(245,158,11,0.3) 0%, rgba(99,102,241,0.15) 40%, transparent 70%)',
        }}
      />

      <div className="relative z-10 text-center pt-24 pb-8 px-4">
        {/* 标签 */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
          className="inline-flex items-center gap-2 px-4 py-1.5 rounded-full border border-amber-500/20 bg-amber-500/5 text-amber-400 text-sm mb-8"
        >
          <span className="w-1.5 h-1.5 rounded-full bg-amber-400 animate-pulse" />
          v0.1.0 Preview
        </motion.div>

        {/* 主标题 */}
        <motion.h1
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6, delay: 0.1 }}
          className="text-6xl md:text-8xl font-black tracking-tight mb-6"
        >
          <span className="gradient-text">衔烛</span>
        </motion.h1>

        {/* 副标题 */}
        <motion.p
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6, delay: 0.2 }}
          className="text-xl md:text-2xl text-white/70 font-medium mb-3 max-w-2xl mx-auto"
        >
          开源免费的桌面 AI 助手，OpenClaw 的开源替代
        </motion.p>

        <motion.p
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6, delay: 0.3 }}
          className="text-base text-white/40 mb-10 max-w-xl mx-auto"
        >
          Open-source desktop AI assistant, a free alternative to OpenClaw
        </motion.p>

        {/* CTA 按钮 */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6, delay: 0.4 }}
          className="flex items-center justify-center gap-4 flex-wrap"
        >
          <a href="#download" className="btn-primary">
            <PlatformIcon />
            下载 {platformLabel} 版
          </a>
          <a
            href="https://github.com/zyswork/xianzhu-claw"
            target="_blank"
            rel="noopener noreferrer"
            className="btn-secondary"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
              <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z"/>
            </svg>
            GitHub
          </a>
        </motion.div>
        <motion.p
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.6, delay: 0.6 }}
          className="text-white/30 text-sm mt-3"
        >
          同时支持 macOS · Windows · Linux &nbsp;|&nbsp; <a href="#download" className="text-amber-400/60 hover:text-amber-400 transition-colors">查看所有平台</a>
        </motion.p>
      </div>

      {/* 产品截图 */}
      <ProductMockup />

      {/* 底部淡出渐变 */}
      <div className="absolute bottom-0 left-0 right-0 h-32 bg-gradient-to-t from-[#0a0a0f] to-transparent pointer-events-none" />
    </section>
  );
}
