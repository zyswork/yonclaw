import { motion } from 'framer-motion';
import { useState, useEffect } from 'react';

/* ===== 目录结构 ===== */
const tocItems = [
  { id: 'install', label: '安装' },
  { id: 'first-config', label: '首次配置' },
  { id: 'create-agent', label: '创建 Agent' },
  { id: 'skills', label: '技能安装' },
  { id: 'multi-agent', label: '多 Agent 协作' },
  { id: 'memory', label: '记忆管理' },
  { id: 'channels', label: '频道接入' },
  { id: 'faq', label: '常见问题' },
];

/* ===== 代码块组件 ===== */
function CodeBlock({ code, lang = 'bash' }: { code: string; lang?: string }) {
  return (
    <div className="rounded-xl border border-white/[0.06] bg-[#12121a]/80 backdrop-blur-sm overflow-hidden my-4">
      <div className="flex items-center gap-2 px-4 py-2 border-b border-white/[0.04] bg-white/[0.02]">
        <div className="w-2.5 h-2.5 rounded-full bg-[#ff5f57]" />
        <div className="w-2.5 h-2.5 rounded-full bg-[#febc2e]" />
        <div className="w-2.5 h-2.5 rounded-full bg-[#28c840]" />
        <span className="ml-2 text-[10px] text-white/20 font-mono">{lang}</span>
      </div>
      <pre className="p-4 text-sm text-white/60 font-mono leading-relaxed overflow-x-auto">
        <code>{code}</code>
      </pre>
    </div>
  );
}

/* ===== 步骤组件 ===== */
function Step({ number, title, children }: { number: number; title: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-4 mb-8">
      <div className="flex-shrink-0 w-8 h-8 rounded-full bg-amber-500/10 border border-amber-500/20 flex items-center justify-center text-amber-400 text-sm font-bold">
        {number}
      </div>
      <div className="flex-1 min-w-0">
        <h4 className="text-white font-semibold mb-2">{title}</h4>
        <div className="text-white/40 text-sm leading-relaxed">{children}</div>
      </div>
    </div>
  );
}

/* ===== 侧边栏 TOC ===== */
function Sidebar({ activeId }: { activeId: string }) {
  return (
    <nav className="hidden lg:block sticky top-24 w-52 flex-shrink-0">
      <div className="text-xs text-white/30 uppercase tracking-wider mb-4 font-semibold">
        目录
      </div>
      <ul className="space-y-1">
        {tocItems.map((item) => (
          <li key={item.id}>
            <a
              href={`#${item.id}`}
              className={`block px-3 py-1.5 text-sm rounded-lg transition-colors ${
                activeId === item.id
                  ? 'text-amber-400 bg-amber-500/10'
                  : 'text-white/40 hover:text-white/60 hover:bg-white/[0.03]'
              }`}
            >
              {item.label}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}

/* ===== 章节组件 ===== */
function Section({ id, title, children }: { id: string; title: string; children: React.ReactNode }) {
  return (
    <section id={id} className="mb-20 scroll-mt-24">
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        whileInView={{ opacity: 1, y: 0 }}
        viewport={{ once: true }}
        transition={{ duration: 0.5 }}
      >
        <h2 className="text-2xl md:text-3xl font-bold mb-6">
          <span className="gradient-text">{title}</span>
        </h2>
        <div className="text-white/50 text-sm leading-relaxed space-y-4">
          {children}
        </div>
      </motion.div>
    </section>
  );
}

/* ===== 主组件 ===== */
export default function Guide() {
  const [activeId, setActiveId] = useState('install');

  useEffect(() => {
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            setActiveId(entry.target.id);
          }
        }
      },
      { rootMargin: '-20% 0px -70% 0px' }
    );

    for (const item of tocItems) {
      const el = document.getElementById(item.id);
      if (el) observer.observe(el);
    }

    return () => observer.disconnect();
  }, []);

  return (
    <div className="pt-24 pb-16 px-4">
      <div className="max-w-5xl mx-auto">
        {/* 页面标题 */}
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.6 }}
          className="text-center mb-16"
        >
          <h1 className="text-4xl md:text-6xl font-bold mb-4">
            使用<span className="gradient-text">手册</span>
          </h1>
          <p className="text-white/40 text-lg">从安装到精通，快速上手衔烛</p>
        </motion.div>

        <div className="flex gap-12">
          <Sidebar activeId={activeId} />

          {/* 内容区 */}
          <div className="flex-1 min-w-0">
            {/* 安装 */}
            <Section id="install" title="安装">
              <p>衔烛支持 macOS、Windows 和 Linux 三大平台。</p>
              <Step number={1} title="下载安装包">
                <p>
                  前往{' '}
                  <a href="https://github.com/zyswork/xianzhu-claw/releases/latest" target="_blank" rel="noopener noreferrer" className="text-amber-400 hover:underline">
                    GitHub Releases
                  </a>{' '}
                  页面，根据你的操作系统选择对应的安装包。
                </p>
                <ul className="list-disc list-inside mt-2 space-y-1">
                  <li><strong className="text-white/60">macOS</strong>: 下载 .dmg 文件，拖入 Applications</li>
                  <li><strong className="text-white/60">Windows</strong>: 下载 .msi 安装包，双击安装</li>
                  <li><strong className="text-white/60">Linux</strong>: 下载 .deb 或 .AppImage</li>
                </ul>
              </Step>
              <Step number={2} title="macOS 安全提示">
                <p>首次打开时如果提示「无法验证开发者」，前往系统设置 &gt; 隐私与安全性 &gt; 点击「仍要打开」。</p>
              </Step>
              <Step number={3} title="Linux 安装">
                <CodeBlock code={`# Debian / Ubuntu
sudo dpkg -i xianzhu_0.1.0_amd64.deb

# AppImage
chmod +x XianZhu-0.1.0.AppImage
./XianZhu-0.1.0.AppImage`} />
              </Step>
            </Section>

            {/* 首次配置 */}
            <Section id="first-config" title="首次配置">
              <p>安装完成后，首次启动需要配置 AI 供应商和 API Key。</p>
              <Step number={1} title="添加供应商">
                <p>进入「设置 &gt; 供应商管理」，点击「添加供应商」，选择你使用的 AI 服务商。</p>
                <p className="mt-2">支持的供应商包括：OpenAI、Anthropic (Claude)、Google (Gemini)、Moonshot (Kimi)、DeepSeek 等。</p>
              </Step>
              <Step number={2} title="配置 API Key（或 OAuth 免费授权）">
                <p>输入对应供应商的 API Key。对于 <strong>Google Gemini</strong>，支持 OAuth 免费授权 — 无需 API Key，直接用 Google 账号登录即可免费使用 Gemini 2.5/3.1 系列模型。</p>
                <p className="mt-2">Key 会加密存储在本地数据库中，不上传任何服务器。</p>
                <CodeBlock code={`# API Key 存储位置
# macOS: ~/Library/Application Support/com.xianzhu.app/xianzhu.db
# Windows: %APPDATA%/com.xianzhu.app/xianzhu.db
# Linux: ~/.local/share/com.xianzhu.app/xianzhu.db`} />
              </Step>
              <Step number={3} title="选择默认模型">
                <p>在供应商配置完成后，进入「设置 &gt; 默认模型」，选择你常用的模型作为默认对话模型。</p>
              </Step>
            </Section>

            {/* 创建 Agent */}
            <Section id="create-agent" title="创建 Agent">
              <p>Agent 是衔烛的核心概念。每个 Agent 有自己的模型、人格和技能配置。</p>
              <Step number={1} title="新建 Agent">
                <p>点击侧栏的「+」按钮，填写 Agent 名称、描述。</p>
              </Step>
              <Step number={2} title="选择模型">
                <p>为 Agent 选择一个或多个可用模型。不同任务可以使用不同模型。</p>
              </Step>
              <Step number={3} title="设置人格">
                <p>编写系统提示词（System Prompt），定义 Agent 的角色、能力范围和回复风格。</p>
                <CodeBlock lang="text" code={`你是一个专业的技术文档撰写助手。
你擅长将复杂的技术概念用简洁清晰的语言解释。
回复时使用中文，代码注释也使用中文。
保持专业但友好的语气。`} />
              </Step>
              <Step number={4} title="开始对话">
                <p>配置完成后，点击 Agent 即可开始对话。对话记录会自动保存。</p>
              </Step>
            </Section>

            {/* 技能安装 */}
            <Section id="skills" title="技能安装">
              <p>技能（Skill）是 Agent 可以调用的工具，如网页搜索、代码执行、文件读写等。</p>
              <Step number={1} title="浏览技能市场">
                <p>进入「技能市场」，浏览可用技能。每个技能有详细说明和权限要求。</p>
              </Step>
              <Step number={2} title="安装技能">
                <p>点击「安装」，技能会下载到本地并在沙箱中运行。</p>
                <CodeBlock code={`# 技能存储位置
~/.xianzhu/skills/

# 目录结构
~/.xianzhu/skills/web_search/
  ├── manifest.json    # 技能描述和配置
  ├── index.js         # 技能逻辑
  └── README.md        # 使用说明`} />
              </Step>
              <Step number={3} title="分配给 Agent">
                <p>在 Agent 设置中勾选需要启用的技能。不同 Agent 可以拥有不同的技能组合。</p>
              </Step>
            </Section>

            {/* 多 Agent 协作 */}
            <Section id="multi-agent" title="多 Agent 协作">
              <p>衔烛支持多个 Agent 之间的协作，实现复杂任务的分工处理。</p>
              <Step number={1} title="创建关系">
                <p>在 Agent 设置中，添加「可委派对象」，建立 Agent 之间的协作关系。</p>
              </Step>
              <Step number={2} title="任务委派">
                <p>在对话中，Agent A 可以将子任务委派给 Agent B。例如：</p>
                <ul className="list-disc list-inside mt-2 space-y-1">
                  <li>搜索 Agent 负责信息检索</li>
                  <li>分析 Agent 负责数据处理</li>
                  <li>写作 Agent 负责内容生成</li>
                </ul>
              </Step>
              <Step number={3} title="智能路由">
                <p>开启智能路由后，系统会根据消息内容自动选择最合适的 Agent 处理。</p>
              </Step>
            </Section>

            {/* 记忆管理 */}
            <Section id="memory" title="记忆管理">
              <p>衔烛的三层记忆系统让 AI 真正理解你。</p>
              <Step number={1} title="记忆类型">
                <ul className="list-disc list-inside space-y-1">
                  <li><strong className="text-white/60">工作记忆</strong>：当前对话的上下文，对话结束后清除</li>
                  <li><strong className="text-white/60">短期记忆</strong>：近期对话的摘要，保留数天到数周</li>
                  <li><strong className="text-white/60">长期记忆</strong>：提取的关键事实和偏好，永久保存</li>
                </ul>
              </Step>
              <Step number={2} title="查看记忆">
                <p>进入「Agent 设置 &gt; 记忆」，可以查看和管理 Agent 的所有记忆条目。</p>
              </Step>
              <Step number={3} title="手动提取">
                <p>在对话中，你可以手动标记重要信息让 AI 记住，也可以删除不需要的记忆。</p>
              </Step>
            </Section>

            {/* 频道接入 */}
            <Section id="channels" title="频道接入">
              <p>将 Agent 接入外部通讯频道，随时随地与 AI 对话。</p>
              <Step number={1} title="支持的频道">
                <ul className="list-disc list-inside space-y-1">
                  <li><strong className="text-white/60">Telegram</strong>：通过 Bot API 接入</li>
                  <li><strong className="text-white/60">Discord</strong>：通过 Bot 接入</li>
                  <li><strong className="text-white/60">飞书</strong>：通过应用机器人接入</li>
                  <li><strong className="text-white/60">微信</strong>：通过 WeChat Bot 协议接入</li>
                </ul>
              </Step>
              <Step number={2} title="配置 Telegram 示例">
                <CodeBlock code={`# 1. 在 Telegram 中找到 @BotFather，创建 Bot 获取 Token
# 2. 在衔烛「频道管理」中添加 Telegram 频道
# 3. 填入 Bot Token
# 4. 选择关联的 Agent
# 5. 保存并启用`} />
              </Step>
              <Step number={3} title="权限控制">
                <p>可以设置允许哪些用户/群组与 Bot 对话，防止未授权访问。</p>
              </Step>
            </Section>

            {/* FAQ */}
            <Section id="faq" title="常见问题">
              <div className="space-y-6">
                <div className="glass-card p-5">
                  <h4 className="text-white font-semibold mb-2">Q: 衔烛是免费的吗？</h4>
                  <p>衔烛本身是免费开源的（MIT License）。但你需要自备 AI 供应商的 API Key，这些 API 的费用由供应商收取。</p>
                </div>
                <div className="glass-card p-5">
                  <h4 className="text-white font-semibold mb-2">Q: 数据存在哪里？</h4>
                  <p>所有数据存储在你的本地设备上，具体位置取决于操作系统。衔烛不会将任何对话数据上传到第三方服务器。</p>
                </div>
                <div className="glass-card p-5">
                  <h4 className="text-white font-semibold mb-2">Q: 支持哪些模型？</h4>
                  <p>支持所有兼容 OpenAI API 格式的模型，包括 GPT-4o、Claude、Gemini、Kimi、DeepSeek、Qwen 等。</p>
                </div>
                <div className="glass-card p-5">
                  <h4 className="text-white font-semibold mb-2">Q: 如何更新衔烛？</h4>
                  <p>衔烛支持自动更新检查。也可以前往 GitHub Releases 手动下载最新版本。</p>
                </div>
                <div className="glass-card p-5">
                  <h4 className="text-white font-semibold mb-2">Q: 遇到问题怎么办？</h4>
                  <p>
                    请在{' '}
                    <a href="https://github.com/zyswork/xianzhu-claw/issues" target="_blank" rel="noopener noreferrer" className="text-amber-400 hover:underline">
                      GitHub Issues
                    </a>{' '}
                    提交问题，附上日志文件（位于 ~/Library/Logs/XianZhu/）可以帮助我们更快定位问题。
                  </p>
                </div>
              </div>
            </Section>
          </div>
        </div>
      </div>
    </div>
  );
}
