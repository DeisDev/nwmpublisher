import { _, addMessages, init, getLocaleFromNavigator, locale } from 'svelte-i18n';
import { get } from 'svelte/store';
import en from '../i18n/en.json';

window.APP_LANGUAGES = JSON.parse(__NWMPUBLISHER_APP_LANGUAGES__);

{
	for (let file in window.APP_LANGUAGES) {
		if (file === 'en') continue;
		addMessages(file, window.APP_LANGUAGES[file]);
	}
	addMessages('en', en);

	const navigatorLocale = getLocaleFromNavigator() ?? 'en';
	init({
		fallbackLocale: 'en',
		initialLocale: navigatorLocale,
	});

	console.report('info', `Initial locale: ${navigatorLocale}`);
}

const RE_SPLIT_ERROR = /^(.*?)(?::([\s\S]*))?$/;
export function translateError(error, data) {
	const match = String(error).match(RE_SPLIT_ERROR);
	const key = match?.[1] ?? String(error);
	const details = data == null ? match?.[2] : (typeof data === 'string' ? data : JSON.stringify(data, null, 2));
	const message = get(_)(key, { values: { data: details ?? '' } });
	return details && !message.includes(details) ? message + '\n' + details : message;
}

export function switchLanguage(switchLocale) {
	const newLocale = switchLocale in window.APP_LANGUAGES ? switchLocale : (getLocaleFromNavigator() ?? 'en');
	locale.set(newLocale);
	console.report('info', `Switched to locale: ${newLocale}`);
}
