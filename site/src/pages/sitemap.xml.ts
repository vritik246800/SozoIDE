// The pages of the site for search engines (TODO 6.4.2): written here, without a dependency.
import type { APIRoute } from 'astro';

const pages = ['', 'install/', 'notes/', 'shortcuts/'];

export const GET: APIRoute = ({ site }) => {
  const base = import.meta.env.BASE_URL.replace(/\/?$/, '/');
  const urls = pages.map((page) => `  <url><loc>${new URL(base + page, site)}</loc></url>`).join('\n');
  return new Response(`<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls}\n</urlset>\n`, {
    headers: { 'Content-Type': 'application/xml' },
  });
};
