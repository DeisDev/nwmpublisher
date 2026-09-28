import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import en from '../i18n/en.json' with { type: 'json' };

export default defineConfig({
	root: fileURLToPath(new URL('.', import.meta.url)),
	base: './',
	publicDir: false,
	plugins: [
		svelte(),
		{
			name: 'site-metadata',
			transformIndexHtml() {
				return [
					{ tag: 'meta', attrs: { name: 'description', content: en.website.description } },
					{ tag: 'meta', attrs: { property: 'og:title', content: 'nwmpublisher' } },
					{ tag: 'meta', attrs: { property: 'og:description', content: en.website.description } },
					{ tag: 'meta', attrs: { property: 'og:type', content: 'website' } }
				];
			}
		}
	],
	build: { outDir: 'dist' }
});
