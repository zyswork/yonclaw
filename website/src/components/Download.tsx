import { motion } from 'framer-motion';
import { useEffect, useState } from 'react';

type Platform = 'macos' | 'windows' | 'linux';

const BASE = 'https://zys-openclaw.com/downloads';
const VER = '0.1.0';

const downloadUrls: Record<Platform, string> = {
  macos: `${BASE}/XianZhu_${VER}_aarch64.dmg`,
  windows: `${BASE}/XianZhu_${VER}_x64_en-US.msi`,
  linux: `${BASE}/xian-zhu_${VER}_amd64.AppImage`,
};

function detectPlatform(): Platform {
  if (typeof navigator === 'undefined') return 'macos';
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('mac')) return 'macos';
  if (ua.includes('win')) return 'windows';
  return 'linux';
}

const platforms: { key: Platform; label: string; desc: string; icon: React.ReactNode }[] = [
  {
    key: 'macos',
    label: 'macOS',
    desc: 'Apple Silicon & Intel',
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
        <path d="M18.71 19.5c-.83 1.24-1.71 2.45-3.05 2.47-1.34.03-1.77-.79-3.29-.79-1.53 0-2 .77-3.27.82-1.31.05-2.3-1.32-3.14-2.53C4.25 17 2.94 12.45 4.7 9.39c.87-1.52 2.43-2.48 4.12-2.51 1.28-.02 2.5.87 3.29.87.78 0 2.26-1.07 3.8-.91.65.03 2.47.26 3.64 1.98-.09.06-2.17 1.28-2.15 3.81.03 3.02 2.65 4.03 2.68 4.04-.03.07-.42 1.44-1.38 2.83M13 3.5c.73-.83 1.94-1.46 2.94-1.5.13 1.17-.34 2.35-1.04 3.19-.69.85-1.83 1.51-2.95 1.42-.15-1.15.41-2.35 1.05-3.11z"/>
      </svg>
    ),
  },
  {
    key: 'windows',
    label: 'Windows',
    desc: 'Windows 10+',
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
        <path d="M3 12V6.75l6-1.32v6.48L3 12zm6.2.13l.01 6.42-6.2-1.01V12.5l6.19-.37zM10.2 5.25l9.8-1.5v8.11l-9.8.1V5.25zm9.79 6.61l.01 8.39-9.8-1.5V12l9.79-.14z"/>
      </svg>
    ),
  },
  {
    key: 'linux',
    label: 'Linux',
    desc: '.deb / .AppImage',
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
        <path d="M12.504 0c-.155 0-.315.008-.48.021-4.226.333-3.105 4.807-3.17 6.298-.076 1.092-.3 1.953-1.05 3.02-.885 1.051-2.127 2.75-2.716 4.521-.278.832-.41 1.684-.287 2.489.117.811.475 1.541 1.139 2.021.603.437 1.376.624 2.271.559 1.418-.103 2.143-.628 3.228-.843.385-.076.788-.12 1.225-.113.436-.007.84.037 1.225.113 1.085.215 1.81.74 3.228.843.895.065 1.668-.122 2.27-.559.665-.48 1.023-1.21 1.14-2.021.123-.805-.01-1.657-.288-2.489-.589-1.771-1.831-3.47-2.715-4.521-.75-1.067-.975-1.928-1.051-3.02-.065-1.491 1.056-5.965-3.17-6.298A4.003 4.003 0 0 0 12.504 0z"/>
      </svg>
    ),
  },
];

export default function Download() {
  const [currentPlatform, setCurrentPlatform] = useState<Platform>('macos');

  useEffect(() => {
    setCurrentPlatform(detectPlatform());
  }, []);

  return (
    <section id="download" className="relative py-32 px-4">
      {/* 背景发光 */}
      <div
        className="absolute inset-0 -z-10 opacity-20 pointer-events-none"
        style={{
          background:
            'radial-gradient(ellipse at center bottom, rgba(245,158,11,0.2), transparent 60%)',
        }}
      />

      <div className="max-w-3xl mx-auto text-center">
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.6 }}
        >
          <h2 className="text-3xl md:text-5xl font-bold mb-4">
            开始使用<span className="gradient-text">衔烛</span>
          </h2>
          <p className="text-white/40 text-lg mb-12">
            永久免费，开源 MIT 许可，立即体验 AI 原生桌面助手
          </p>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.6, delay: 0.2 }}
          className="flex flex-col sm:flex-row gap-4 justify-center"
        >
          {platforms.map((p) => {
            const isCurrent = p.key === currentPlatform;
            return (
              <a
                key={p.key}
                href={downloadUrls[p.key]}
                target="_blank"
                rel="noopener noreferrer"
                className={`
                  group flex items-center gap-4 px-8 py-5 rounded-2xl border transition-all duration-300
                  ${
                    isCurrent
                      ? 'bg-gradient-to-r from-amber-500/10 to-orange-500/10 border-amber-500/30 glow-amber'
                      : 'bg-white/[0.02] border-white/[0.08] hover:border-white/20'
                  }
                `}
              >
                <div className={isCurrent ? 'text-amber-400' : 'text-white/40 group-hover:text-white/60'}>
                  {p.icon}
                </div>
                <div className="text-left">
                  <div className={`font-semibold ${isCurrent ? 'text-white' : 'text-white/70'}`}>
                    {p.label}
                  </div>
                  <div className="text-xs text-white/30">{p.desc}</div>
                </div>
                {isCurrent && (
                  <span className="ml-auto text-xs text-amber-400 bg-amber-400/10 px-2 py-0.5 rounded-full">
                    推荐
                  </span>
                )}
              </a>
            );
          })}
        </motion.div>

        <motion.p
          initial={{ opacity: 0 }}
          whileInView={{ opacity: 1 }}
          viewport={{ once: true }}
          transition={{ duration: 0.6, delay: 0.4 }}
          className="text-white/20 text-sm mt-8"
        >
          v0.1.0 Preview &middot;{' '}
          <a
            href="https://github.com/zyswork/xianzhu-claw/releases"
            target="_blank"
            rel="noopener noreferrer"
            className="underline hover:text-white/40 transition-colors"
          >
            查看所有版本
          </a>
        </motion.p>
      </div>
    </section>
  );
}
