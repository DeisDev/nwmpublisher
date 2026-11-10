const themeStorageKey = 'nwmpublisher.theme';
const themes = ['system', 'light', 'dark'];
const root = document.documentElement;
const themeColors = document.querySelectorAll('meta[name="theme-color"]');
const systemColors = Array.from(themeColors, meta => meta.content);

export function savedTheme() {
	return themes.includes(root.dataset.theme) ? root.dataset.theme : 'system';
}

export function nextTheme(theme) {
	return themes[(themes.indexOf(theme) + 1) % themes.length];
}

export function applyTheme(theme) {
	root.dataset.theme = theme;
	const background = getComputedStyle(root).backgroundColor;
	themeColors.forEach((meta, index) => meta.content = theme === 'system' ? systemColors[index] : background);
}

export function saveTheme(theme) {
	try {
		if (theme === 'system') localStorage.removeItem(themeStorageKey);
		else localStorage.setItem(themeStorageKey, theme);
	} catch (error) {
		console.warn('Failed to save the theme:', error);
	}
}
