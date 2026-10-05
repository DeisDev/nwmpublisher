import { getCurrentWindow } from '@tauri-apps/api/window';

const systemDark = window.matchMedia('(prefers-color-scheme: dark)');
let preference = 'system';

function updateTheme() {
	const dark = preference === 'system' ? systemDark.matches : preference === 'dark';
	document.documentElement.dataset.theme = dark ? 'dark' : 'light';
}

export function applyTheme(theme) {
	preference = ['dark', 'light'].includes(theme) ? theme : 'system';
	updateTheme();
	getCurrentWindow().setTheme(preference === 'system' ? null : preference)
		.catch(error => console.warn('Failed to set the window theme:', error));
}

systemDark.addEventListener('change', updateTheme);
