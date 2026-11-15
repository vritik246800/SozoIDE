// robots.txt with the address of the sitemap, on GitHub Pages and on Vercel (TODO 6.4.2).
import type { APIRoute } from 'astro';

export const GET: APIRoute = ({ site }) => {
  const base = import.meta.env.BASE_URL.replace(/\/$/, '');
  return new Response(`User-agent: *\nAllow: /\n\nSitemap: ${new URL(`${base}/sitemap.xml`, site)}\n`);
};
