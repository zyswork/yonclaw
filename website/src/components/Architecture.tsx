import { motion } from 'framer-motion';

interface TechBadge {
  name: string;
  icon: React.ReactNode;
  color: string;
}

const techs: TechBadge[] = [
  {
    name: 'Rust',
    color: 'rgba(222,165,132,0.15)',
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="#dea584">
        <path d="M23.835 11.703a.732.732 0 0 0-.476-.217l-.539-.056a9.36 9.36 0 0 0-.447-1.08l.322-.43a.732.732 0 0 0-.053-.522.73.73 0 0 0-.391-.349 9.456 9.456 0 0 0-.842-.57l-.007-.003.102-.529a.735.735 0 0 0-.157-.507.729.729 0 0 0-.464-.258 10.14 10.14 0 0 0-.96-.275l.002-.005-.124-.527a.735.735 0 0 0-.307-.436.727.727 0 0 0-.521-.11c-.342.06-.683.14-1.019.237l-.002-.003-.339-.407a.733.733 0 0 0-.932-.137c-.317.175-.626.37-.926.58l-.541-.262a.732.732 0 0 0-.914.225c-.24.28-.464.576-.67.882l-.003-.001-.531-.03a.729.729 0 0 0-.533.183.737.737 0 0 0-.238.494c-.043.356-.066.714-.068 1.074l-.002.001-.405.343a.735.735 0 0 0-.253.482.73.73 0 0 0 .113.521c.178.29.374.568.587.833l-.247.476a.735.735 0 0 0 .014.594c.123.252.26.498.41.736l-.001.002-.07.537a.737.737 0 0 0 .18.542c.24.274.5.532.775.77l.002.003.136.527a.732.732 0 0 0 .362.435c.28.16.57.303.868.428l.323.427a.73.73 0 0 0 .507.285 9.9 9.9 0 0 0 .968.15l.481.277a.732.732 0 0 0 .562.047c.33-.104.656-.228.974-.369l.006.003.536.108a.726.726 0 0 0 .52-.085c.307-.18.605-.378.89-.592l.568-.064a.734.734 0 0 0 .434-.252c.228-.285.44-.584.634-.895h.004l.474-.256a.734.734 0 0 0 .322-.411c.123-.34.228-.688.313-1.04l.003-.002.347-.408a.733.733 0 0 0 .166-.524 9.771 9.771 0 0 0-.036-1.078l.182-.507a.733.733 0 0 0-.018-.527zM12 18.592a6.592 6.592 0 1 1 0-13.184 6.592 6.592 0 0 1 0 13.184z"/>
      </svg>
    ),
  },
  {
    name: 'Tauri',
    color: 'rgba(255,200,60,0.15)',
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="#ffc83c">
        <circle cx="15" cy="6.5" r="3" />
        <circle cx="9" cy="17.5" r="3" />
        <path d="M16.5 9.5C14 14 10 19 7.5 14.5" stroke="#ffc83c" strokeWidth="1.5" fill="none" />
      </svg>
    ),
  },
  {
    name: 'React',
    color: 'rgba(97,218,251,0.15)',
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="#61dafb">
        <circle cx="12" cy="12" r="2.5" />
        <ellipse cx="12" cy="12" rx="10" ry="4" stroke="#61dafb" strokeWidth="1" fill="none" />
        <ellipse cx="12" cy="12" rx="10" ry="4" stroke="#61dafb" strokeWidth="1" fill="none" transform="rotate(60 12 12)" />
        <ellipse cx="12" cy="12" rx="10" ry="4" stroke="#61dafb" strokeWidth="1" fill="none" transform="rotate(120 12 12)" />
      </svg>
    ),
  },
  {
    name: 'TypeScript',
    color: 'rgba(49,120,198,0.15)',
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <rect x="2" y="2" width="20" height="20" rx="3" fill="#3178c6" />
        <text x="12" y="17" textAnchor="middle" fontSize="12" fontWeight="700" fill="white">TS</text>
      </svg>
    ),
  },
  {
    name: 'SQLite',
    color: 'rgba(0,150,200,0.15)',
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#0096c8" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
        <ellipse cx="12" cy="5" rx="9" ry="3" />
        <path d="M3 5v14c0 1.657 4.03 3 9 3s9-1.343 9-3V5" />
        <path d="M3 12c0 1.657 4.03 3 9 3s9-1.343 9-3" />
      </svg>
    ),
  },
];

export default function Architecture() {
  return (
    <section className="relative py-24 px-4">
      <div className="divider-gradient mb-24" />
      <div className="max-w-4xl mx-auto">
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ duration: 0.6 }}
          className="text-center mb-16"
        >
          <h2 className="text-3xl md:text-5xl font-bold mb-4">
            现代<span className="gradient-text-indigo">技术栈</span>
          </h2>
          <p className="text-white/40 text-lg">
            高性能原生体验，内存占用不到 Electron 的 1/3
          </p>
        </motion.div>

        <div className="flex flex-wrap justify-center gap-6">
          {techs.map((tech, i) => (
            <motion.div
              key={tech.name}
              initial={{ opacity: 0, scale: 0.8 }}
              whileInView={{ opacity: 1, scale: 1 }}
              viewport={{ once: true }}
              transition={{ duration: 0.4, delay: i * 0.08 }}
              whileHover={{ scale: 1.05, y: -4 }}
              className="flex items-center gap-3 px-6 py-4 rounded-2xl border border-white/[0.06] bg-white/[0.02] backdrop-blur-sm cursor-default"
              style={{ boxShadow: `0 0 20px ${tech.color}` }}
            >
              {tech.icon}
              <span className="text-white/70 font-medium">{tech.name}</span>
            </motion.div>
          ))}
        </div>
      </div>
    </section>
  );
}
