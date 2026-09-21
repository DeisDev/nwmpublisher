import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import fs from 'fs';

const appLanguages = {};
{
	const languageFiles = fs.readdirSync('./i18n');
	let i = -1;
	while (++i < languageFiles.length) {
		const file = languageFiles[i];
		const fileName = file.substr(0, file.length - 5);
		const languageData = JSON.parse(fs.readFileSync('./i18n/' + file, { encoding: 'utf-8' }));
		appLanguages[fileName] = languageData;
	}
}

export default defineConfig({
	plugins: [svelte()],

	// Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
	//
	// 1. prevent vite from obscuring rust errors
	clearScreen: false,
	// 2. tauri expects a fixed port, fail if that port is not available
	server: {
		port: 1420,
		strictPort: true,
		watch: {
			// 3. tell vite to ignore watching `src-tauri`
			ignored: ["**/src-tauri/**"],
		},
	},
	root: "app",
	publicDir: "../public",
	build: {
		outDir: "../dist",
		emptyOutDir: true,
	},
	define: {
		'__NWMPUBLISHER_APP_LANGUAGES__': JSON.stringify(JSON.stringify(appLanguages))
	}
});
