import { motion } from 'framer-motion';
import { useState } from 'react';

/* ===== 联系信息 ===== */
function ContactInfo() {
  const items = [
    {
      icon: (
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/>
          <circle cx="12" cy="7" r="4"/>
        </svg>
      ),
      label: '作者',
      value: '张永顺',
    },
    {
      icon: (
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z"/>
          <polyline points="22,6 12,13 2,6"/>
        </svg>
      ),
      label: '邮箱',
      value: 'zys_work@outlook.com',
      href: 'mailto:zys_work@outlook.com',
    },
    {
      icon: (
        <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
          <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z"/>
        </svg>
      ),
      label: 'GitHub',
      value: 'github.com/zyswork/xianzhu',
      href: 'https://github.com/zyswork/xianzhu',
      external: true,
    },
    {
      icon: (
        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="12" cy="12" r="10"/>
          <line x1="12" y1="8" x2="12" y2="12"/>
          <line x1="12" y1="16" x2="12.01" y2="16"/>
        </svg>
      ),
      label: '意见反馈',
      value: 'GitHub Issues',
      href: 'https://github.com/zyswork/xianzhu/issues',
      external: true,
    },
  ];

  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
      {items.map((item, i) => (
        <motion.div
          key={i}
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.4, delay: i * 0.08 }}
          className="glass-card p-5 flex items-center gap-4"
        >
          <div className="text-amber-400 flex-shrink-0">{item.icon}</div>
          <div>
            <div className="text-xs text-white/30 mb-0.5">{item.label}</div>
            {item.href ? (
              <a
                href={item.href}
                target={item.external ? '_blank' : undefined}
                rel={item.external ? 'noopener noreferrer' : undefined}
                className="text-white/70 hover:text-amber-400 transition-colors text-sm"
              >
                {item.value}
              </a>
            ) : (
              <span className="text-white/70 text-sm">{item.value}</span>
            )}
          </div>
        </motion.div>
      ))}
    </div>
  );
}

/* ===== 联系表单 ===== */
function ContactForm() {
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [message, setMessage] = useState('');

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const subject = encodeURIComponent(`[衔烛反馈] 来自 ${name}`);
    const body = encodeURIComponent(`姓名: ${name}\n邮箱: ${email}\n\n${message}`);
    window.location.href = `mailto:zys_work@outlook.com?subject=${subject}&body=${body}`;
  };

  return (
    <motion.form
      initial={{ opacity: 0, y: 20 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.5, delay: 0.2 }}
      onSubmit={handleSubmit}
      className="glass-card p-6 space-y-4"
    >
      <h3 className="text-lg font-semibold text-white mb-2">发送消息</h3>
      <div>
        <label className="block text-xs text-white/30 mb-1.5">姓名</label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
          className="w-full px-4 py-2.5 rounded-xl bg-white/[0.03] border border-white/[0.08] text-white/80 text-sm placeholder:text-white/20 focus:outline-none focus:border-amber-500/30 transition-colors"
          placeholder="你的名字"
        />
      </div>
      <div>
        <label className="block text-xs text-white/30 mb-1.5">邮箱</label>
        <input
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          required
          className="w-full px-4 py-2.5 rounded-xl bg-white/[0.03] border border-white/[0.08] text-white/80 text-sm placeholder:text-white/20 focus:outline-none focus:border-amber-500/30 transition-colors"
          placeholder="your@email.com"
        />
      </div>
      <div>
        <label className="block text-xs text-white/30 mb-1.5">消息</label>
        <textarea
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          required
          rows={5}
          className="w-full px-4 py-2.5 rounded-xl bg-white/[0.03] border border-white/[0.08] text-white/80 text-sm placeholder:text-white/20 focus:outline-none focus:border-amber-500/30 transition-colors resize-none"
          placeholder="你想说的话..."
        />
      </div>
      <button
        type="submit"
        className="btn-primary w-full justify-center"
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <line x1="22" y1="2" x2="11" y2="13" />
          <polygon points="22 2 15 22 11 13 2 9 22 2" />
        </svg>
        发送（通过邮件客户端）
      </button>
    </motion.form>
  );
}

/* ===== 社交链接 ===== */
function SocialLinks() {
  const links = [
    {
      name: 'GitHub',
      href: 'https://github.com/zyswork/xianzhu',
      icon: (
        <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
          <path d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z"/>
        </svg>
      ),
    },
    {
      name: 'GitHub Discussions',
      href: 'https://github.com/zyswork/xianzhu/discussions',
      icon: (
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>
        </svg>
      ),
    },
  ];

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      whileInView={{ opacity: 1, y: 0 }}
      viewport={{ once: true }}
      transition={{ duration: 0.5, delay: 0.3 }}
      className="glass-card p-6"
    >
      <h3 className="text-lg font-semibold text-white mb-4">加入社区</h3>
      <p className="text-white/40 text-sm mb-6">
        加入衔烛的开源社区，获取最新动态、提交功能建议、参与开发。
      </p>
      <div className="flex flex-wrap gap-3">
        {links.map((link) => (
          <a
            key={link.name}
            href={link.href}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-2 px-4 py-2.5 rounded-xl border border-white/[0.08] bg-white/[0.02] text-white/60 hover:text-white/90 hover:border-white/20 transition-all text-sm"
          >
            {link.icon}
            {link.name}
          </a>
        ))}
      </div>
    </motion.div>
  );
}

/* ===== 主组件 ===== */
export default function Contact() {
  return (
    <div className="pt-24 pb-16 px-4">
      <div className="max-w-4xl mx-auto">
        {/* 页面标题 */}
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6 }}
          className="text-center mb-16"
        >
          <h1 className="text-4xl md:text-6xl font-bold mb-4">
            联系<span className="gradient-text">我们</span>
          </h1>
          <p className="text-white/40 text-lg">问题反馈、功能建议、合作洽谈</p>
        </motion.div>

        {/* 联系信息 */}
        <div className="mb-12">
          <ContactInfo />
        </div>

        {/* 表单 + 社交 */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          <ContactForm />
          <div className="space-y-6">
            <SocialLinks />
          </div>
        </div>
      </div>
    </div>
  );
}
