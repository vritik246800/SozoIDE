import { defineConfig } from 'astro/config';

// GitHub Pages serves the site under /SozoIDE; Vercel (VERCEL=1 at build time) serves it at the root.
const vercel = process.env.VERCEL === '1';
const domain = process.env.VERCEL_PROJECT_PRODUCTION_URL;

export default defineConfig({
  site: vercel && domain ? `https://${domain}` : 'https://vritik246800.github.io',
  base: vercel ? '/' : '/SozoIDE',
});
