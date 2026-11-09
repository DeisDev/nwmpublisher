import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import en from './i18n/en.json' with { type: 'json' };
import { releasesUrl } from './downloads.js';

const name = 'nwmpublisher';
const siteUrl = 'https://nwmpublisher.deis.me/';
const socialCard = 'social-card.png';
const { website } = en;

const structuredData = [
	{ '@context': 'https://schema.org', '@type': 'WebSite', name, url: siteUrl },
	{
		'@context': 'https://schema.org',
		'@type': 'SoftwareApplication',
		name,
		description: website.description,
		url: siteUrl,
		applicationCategory: 'UtilitiesApplication',
		operatingSystem: 'Windows, macOS, Linux',
		offers: { '@type': 'Offer', price: 0 },
		downloadUrl: releasesUrl,
		isBasedOn: 'https://github.com/WilliamVenner/gmpublisher'
	}
];

const metadata = [
	{ tag: 'title', children: website.title },
	{ tag: 'meta', attrs: { name: 'description', content: website.description } },
	{ tag: 'link', attrs: { rel: 'canonical', href: siteUrl } },
	{ tag: 'meta', attrs: { property: 'og:type', content: 'website' } },
	{ tag: 'meta', attrs: { property: 'og:url', content: siteUrl } },
	{ tag: 'meta', attrs: { property: 'og:site_name', content: name } },
	{ tag: 'meta', attrs: { property: 'og:title', content: website.title } },
	{ tag: 'meta', attrs: { property: 'og:description', content: website.description } },
	{ tag: 'meta', attrs: { property: 'og:image', content: new URL(socialCard, siteUrl).href } },
	{ tag: 'meta', attrs: { property: 'og:image:type', content: 'image/png' } },
	{ tag: 'meta', attrs: { property: 'og:image:width', content: '1200' } },
	{ tag: 'meta', attrs: { property: 'og:image:height', content: '630' } },
	{ tag: 'meta', attrs: { property: 'og:image:alt', content: website.alt_social_card } },
	{ tag: 'meta', attrs: { name: 'twitter:card', content: 'summary_large_image' } },
	{ tag: 'script', attrs: { type: 'application/ld+json' }, children: JSON.stringify(structuredData) }
].map(tag => ({ ...tag, injectTo: 'head' }));

export default defineConfig({
	root: fileURLToPath(new URL('.', import.meta.url)),
	base: './',
	publicDir: false,
	plugins: [
		svelte(),
		{
			name: 'site-metadata',
			transformIndexHtml: () => metadata,
			generateBundle() {
				this.emitFile({
					type: 'asset',
					fileName: socialCard,
					source: readFileSync(new URL('../public/img/social-card.png', import.meta.url))
				});
			}
		}
	],
	build: { outDir: 'dist' }
});
