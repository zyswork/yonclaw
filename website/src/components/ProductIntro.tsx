import { motion, useInView } from 'framer-motion';
import { useRef } from 'react';

/* ===== 产品概述 ===== */
function Overview() {
  return (
    <section className="py-24 px-4">
      <div className="max-w-4xl mx-auto text-center">
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.6 }}
        >
          <h1 className="text-4xl md:text-6xl font-bold mb-6">
            <span className="gradient-text">衔烛</span>是什么
          </h1>
          <p className="text-lg md:text-xl text-white/60 leading-relaxed max-w-3xl mx-auto mb-8">
            衔烛（XianZhu）是一款开源免费的 AI 原生桌面助手，OpenClaw 的开源替代品。基于 Tauri 构建，支持多供应商大模型（含丰富国产模型）、
            多智能体协作、持久化记忆和云端技能市场。它运行在你的本地设备上，数据完全由你掌控。
          </p>
          <p className="text-base text-white/40 leading-relaxed max-w-2xl mx-auto">
            完全开源（MIT License），永久免费。原生支持通义千问、智谱GLM、Kimi、MiniMax、DeepSeek 等国产模型，
            中文优先体验，Google Gemini 免费 OAuth 接入。无需在不同工具间切换，也无需将数据上传到第三方云服务。
          </p>
        </motion.div>
      </div>
    </section>
  );
}

/* ===== 核心优势 ===== */
const advantages = [
  {
    icon: (
      <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <rect x="3" y="11" width="18" height="11" rx="2" ry="2"/>
        <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
      </svg>
    ),
    title: '本地优先，隐私安全',
    desc: '所有数据存储在你的设备上，对话记录、记忆、配置均不上传。API Key 本地加密保存，端到端安全传输。你的数据，始终由你掌控。',
  },
  {
    icon: (
      <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="12" cy="12" r="3"/>
        <path d="M12 1v4M12 19v4M4.22 4.22l2.83 2.83M16.95 16.95l2.83 2.83M1 12h4M19 12h4M4.22 19.78l2.83-2.83M16.95 7.05l2.83-2.83"/>
      </svg>
    ),
    title: '多模型自由切换',
    desc: '支持 OpenAI、Claude、Gemini、Kimi、DeepSeek 等 10+ 供应商。同一对话可切换模型，不被任何供应商锁定，永远选择最适合的模型。',
  },
  {
    icon: (
      <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/>
        <circle cx="9" cy="7" r="4"/>
        <path d="M23 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75"/>
      </svg>
    ),
    title: '多智能体协作',
    desc: '创建多个 Agent，赋予不同人格和能力。Agent 之间可以对话、委派任务、智能路由。复杂任务分而治之，效率倍增。',
  },
  {
    icon: (
      <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M12 2a10 10 0 0 1 10 10c0 5.523-4.477 10-10 10S2 17.523 2 12"/>
        <path d="M2 12h4l3-9 4 18 3-9h4"/>
      </svg>
    ),
    title: '持久化记忆',
    desc: '三层记忆架构：工作记忆、短期记忆、长期记忆。AI 会记住你的偏好、项目上下文、历史经验。越用越懂你，真正的个人 AI 助手。',
  },
];

function CoreAdvantages() {
  const ref = useRef<HTMLDivElement>(null);
  const isInView = useInView(ref, { once: true, margin: '-80px' });

  return (
    <section className="py-24 px-4">
      <div className="max-w-6xl mx-auto">
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.6 }}
          className="text-center mb-16"
        >
          <h2 className="text-3xl md:text-5xl font-bold mb-4">
            核心<span className="gradient-text">优势</span>
          </h2>
          <p className="text-white/40 text-lg">为什么选择衔烛</p>
        </motion.div>

        <div ref={ref} className="grid grid-cols-1 md:grid-cols-2 gap-6">
          {advantages.map((adv, i) => (
            <motion.div
              key={i}
              initial={{ opacity: 0, y: 40 }}
              animate={isInView ? { opacity: 1, y: 0 } : {}}
              transition={{ duration: 0.6, delay: i * 0.1, ease: [0.22, 1, 0.36, 1] }}
              className="glass-card p-8"
            >
              <div className="text-amber-400 mb-4">{adv.icon}</div>
              <h3 className="text-xl font-semibold text-white mb-3">{adv.title}</h3>
              <p className="text-white/40 text-sm leading-relaxed">{adv.desc}</p>
            </motion.div>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ===== 适用场景 ===== */
const scenarios = [
  {
    icon: (
      <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/>
        <circle cx="12" cy="7" r="4"/>
      </svg>
    ),
    title: '个人 AI 助手',
    desc: '日程管理、知识检索、写作辅助、翻译对话，一个助手搞定日常所有 AI 需求。',
  },
  {
    icon: (
      <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M16 18l2-2-2-2M8 18l-2-2 2-2M14 4l-4 16"/>
      </svg>
    ),
    title: '开发辅助',
    desc: '代码生成、架构分析、Bug 调试、文档撰写。支持代码执行技能，直接在助手中运行代码。',
  },
  {
    icon: (
      <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/>
        <circle cx="9" cy="7" r="4"/>
        <path d="M23 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75"/>
      </svg>
    ),
    title: '团队协作',
    desc: '多 Agent 分工协作，一个负责搜索，一个负责分析，一个负责写作。团队效率指数级提升。',
  },
  {
    icon: (
      <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
      </svg>
    ),
    title: '客服自动化',
    desc: '接入 Telegram、Discord、飞书、微信频道，让 AI Agent 自动回复用户消息，7x24 在线。',
  },
];

function Scenarios() {
  return (
    <section className="py-24 px-4">
      <div className="max-w-6xl mx-auto">
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.6 }}
          className="text-center mb-16"
        >
          <h2 className="text-3xl md:text-5xl font-bold mb-4">
            适用<span className="gradient-text-indigo">场景</span>
          </h2>
          <p className="text-white/40 text-lg">覆盖你的多种 AI 使用需求</p>
        </motion.div>

        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
          {scenarios.map((s, i) => (
            <motion.div
              key={i}
              initial={{ opacity: 0, y: 30 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true }}
              transition={{ duration: 0.5, delay: i * 0.08 }}
              className="glass-card p-6 text-center"
            >
              <div className="text-amber-400 mb-4 flex justify-center">{s.icon}</div>
              <h3 className="text-base font-semibold text-white mb-2">{s.title}</h3>
              <p className="text-white/40 text-sm leading-relaxed">{s.desc}</p>
            </motion.div>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ===== 对比优势 ===== */
const comparisons = [
  {
    vs: 'OpenClaw',
    advantages: [
      '完全开源 MIT License，OpenClaw 仅部分开源',
      '永久免费使用，无需付费订阅',
      '原生支持 10+ 国产模型供应商',
      '中文优先体验，非英文为主',
      '数据完全本地存储，非云端同步',
    ],
  },
  {
    vs: 'ChatGPT',
    advantages: [
      '数据本地存储，无隐私泄露风险',
      '支持 10+ 供应商，不被 OpenAI 锁定',
      '持久化记忆，跨会话记住上下文',
      '预置 60+ 技能 + 云端市场海量扩展',
    ],
  },
  {
    vs: 'Cursor',
    advantages: [
      '不限于编程场景，通用 AI 助手',
      '多智能体协作，不只是单模型对话',
      '支持 Telegram/Discord 等多渠道接入',
      '可自定义 Agent 人格和技能组合',
    ],
  },
];

function Comparison() {
  return (
    <section className="py-24 px-4">
      <div className="max-w-5xl mx-auto">
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.6 }}
          className="text-center mb-16"
        >
          <h2 className="text-3xl md:text-5xl font-bold mb-4">
            对比<span className="gradient-text">优势</span>
          </h2>
          <p className="text-white/40 text-lg">衔烛 vs 竞品</p>
        </motion.div>

        {/* 衔烛 vs OpenClaw 详细对比表格 */}
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.5 }}
          className="glass-card overflow-hidden mb-12"
        >
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-white/[0.06]">
                <th className="text-left px-6 py-4 text-white/40 font-medium"></th>
                <th className="text-center px-6 py-4 text-amber-400 font-semibold">衔烛Claw XianZhuClaw</th>
                <th className="text-center px-6 py-4 text-white/50 font-semibold">OpenClaw</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/[0.04]">
              {[
                ['开源', 'MIT License', '部分开源'],
                ['价格', '免费', '付费订阅'],
                ['国产模型', '10+ 供应商', '有限支持'],
                ['中文体验', '原生中文', '英文为主'],
                ['本地数据', '完全本地', '云端同步'],
                ['Gemini', '免费 OAuth 接入', '需 API Key'],
              ].map(([label, xz, oc], i) => (
                <tr key={i} className="hover:bg-white/[0.02] transition-colors">
                  <td className="px-6 py-3 text-white/50 font-medium">{label}</td>
                  <td className="px-6 py-3 text-center text-amber-400/80">{xz}</td>
                  <td className="px-6 py-3 text-center text-white/30">{oc}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </motion.div>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
          {comparisons.map((comp, i) => (
            <motion.div
              key={i}
              initial={{ opacity: 0, y: 30 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true }}
              transition={{ duration: 0.5, delay: i * 0.1 }}
              className="glass-card p-6"
            >
              <h3 className="text-lg font-semibold text-white mb-1">
                衔烛 <span className="text-white/30">vs</span>{' '}
                <span className="text-amber-400">{comp.vs}</span>
              </h3>
              <div className="divider-gradient my-4" />
              <ul className="space-y-3">
                {comp.advantages.map((adv, j) => (
                  <li key={j} className="flex items-start gap-2 text-sm text-white/50">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#f59e0b" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="flex-shrink-0 mt-0.5">
                      <polyline points="20 6 9 17 4 12"/>
                    </svg>
                    {adv}
                  </li>
                ))}
              </ul>
            </motion.div>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ===== 主组件 ===== */
export default function ProductIntro() {
  return (
    <div className="pt-16">
      <Overview />
      <div className="divider-gradient" />
      <CoreAdvantages />
      <div className="divider-gradient" />
      <Scenarios />
      <div className="divider-gradient" />
      <Comparison />
    </div>
  );
}
