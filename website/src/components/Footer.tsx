import { motion } from 'framer-motion';

export default function Footer() {
  return (
    <footer className="relative py-16 px-4">
      <div className="divider-gradient mb-16" />
      <div className="max-w-4xl mx-auto text-center">
        {/* 标语 */}
        <motion.p
          initial={{ opacity: 0 }}
          whileInView={{ opacity: 1 }}
          viewport={{ once: true }}
          transition={{ duration: 0.8 }}
          className="text-xl font-medium text-white/20 mb-12 tracking-widest"
        >
          衔火而行，烛照前路
        </motion.p>

        {/* 导航链接 */}
        <div className="flex items-center justify-center gap-6 text-sm text-white/30 mb-6 flex-wrap">
          <a href="/product" className="hover:text-white/60 transition-colors">产品介绍</a>
          <span className="w-1 h-1 rounded-full bg-white/10 hidden sm:block" />
          <a href="/guide" className="hover:text-white/60 transition-colors">使用手册</a>
          <span className="w-1 h-1 rounded-full bg-white/10 hidden sm:block" />
          <a href="/#download" className="hover:text-white/60 transition-colors">下载</a>
          <span className="w-1 h-1 rounded-full bg-white/10 hidden sm:block" />
          <a href="/contact" className="hover:text-white/60 transition-colors">联系我们</a>
        </div>

        {/* 外部链接 */}
        <div className="flex items-center justify-center gap-8 text-sm text-white/30 mb-8">
          <a
            href="https://github.com/zyswork/xianzhu-claw"
            target="_blank"
            rel="noopener noreferrer"
            className="hover:text-white/60 transition-colors"
          >
            GitHub
          </a>
          <span className="w-1 h-1 rounded-full bg-white/10" />
          <a
            href="https://github.com/zyswork/xianzhu-claw/issues"
            target="_blank"
            rel="noopener noreferrer"
            className="hover:text-white/60 transition-colors"
          >
            意见反馈
          </a>
          <span className="w-1 h-1 rounded-full bg-white/10" />
          <span>MIT License</span>
        </div>

        {/* 作者 */}
        <p className="text-xs text-white/15">
          Made by 张永顺 &middot;{' '}
          <a
            href="mailto:zys_work@outlook.com"
            className="hover:text-white/30 transition-colors"
          >
            zys_work@outlook.com
          </a>
        </p>
      </div>
    </footer>
  );
}
