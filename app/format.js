import filesize from 'filesize';
import { locale } from 'svelte-i18n';
import { get } from 'svelte/store';

export function formatSize(bytes, selectedLocale = get(locale)) {
	// The Korean translation uses "kr" as its application language key.
	const numberLocale = selectedLocale === 'kr' ? 'ko' : (selectedLocale ?? 'en');
	return filesize(bytes, {
		base: 2,
		standard: 'iec',
		round: 2,
		locale: numberLocale,
	});
}

export function formatRate(bytesPerSecond, selectedLocale = get(locale)) {
	return formatSize(bytesPerSecond, selectedLocale) + '/s';
}
