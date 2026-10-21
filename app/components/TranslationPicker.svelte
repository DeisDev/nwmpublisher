<script>
	import { _, locale } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import { translateError } from '../i18n';

	export let addonId = null;
	export let contentPath = null;
	export let active = false;
	export let disabled = false;
	export let language = '';
	export let drafts = {};
	export let current = null;
	export let languageName = '';
	export let descriptionError = null;
	export let busy = false;
	export let invalid = null;

	const titleMaxBytes = 128;
	const descriptionMaxBytes = 7999;
	const encoder = new TextEncoder();
	let languages = [];
	let languagesError = null;
	let key = null;
	let context = null;
	let addonContext;
	let draftsRequest = 0;
	let draftsLoading = false;
	let error = null;
	let loaded = {};
	let pending = {};
	let failed = {};
	let saving = Promise.resolve();
	let saveRevision = 0;
	let pendingSaves = 0;
	let saveError = null;

	invoke('workshop_languages').then(list => languages = list, err => languagesError = err);

	$: syncSession(active, addonId, addonId ? null : contentPath);
	$: names = new Intl.DisplayNames([$locale ?? 'en'], { type: 'language' });
	$: options = languages.map(([code, tag]) => ({ code, name: names.of(tag) })).sort((a, b) => a.name.localeCompare(b.name, $locale ?? 'en'));
	$: languageName = options.find(option => option.code === language)?.name ?? '';
	$: current = language ? drafts[language] ?? loaded[language] ?? null : null;
	$: titleError = current && titleProblem(current.title);
	$: descriptionError = current && descriptionProblem(current.description);
	$: invalid = Object.values(drafts).map(text => titleProblem(text.title) ?? descriptionProblem(text.description)).find(Boolean) ?? null;
	$: busy = draftsLoading || !key || !!error || !!saveError || !!invalid;

	function titleProblem(title) {
		return encoder.encode(title).length > titleMaxBytes ? 'ERR_TITLE_TOO_LONG' : title.includes('\0') ? 'ERR_TITLE_CONTAINS_NUL' : null;
	}

	function descriptionProblem(description) {
		return encoder.encode(description).length > descriptionMaxBytes ? 'ERR_DESCRIPTION_TOO_LONG' : description.includes('\0') ? 'ERR_DESCRIPTION_CONTAINS_NUL' : null;
	}

	function without(object, field) {
		const { [field]: _removed, ...rest } = object;
		return rest;
	}

	function syncSession(open, id, path) {
		const next = JSON.stringify([id, path]);
		if (!open || next === context) return;
		if (id !== addonContext) {
			addonContext = id;
			key = null;
			drafts = {};
			language = '';
			loaded = {};
			pending = {};
			failed = {};
		}
		context = next;
		loadDrafts(id, path);
	}

	async function loadDrafts(id = addonId, path = addonId ? null : contentPath) {
		const request = ++draftsRequest;
		const carried = key === 'new' && !id ? drafts : {};
		draftsLoading = true;
		error = null;
		try {
			await flush();
			const next = await invoke('get_translations', { addonId: id, contentPath: path });
			if (request !== draftsRequest) return;
			// Translations typed before choosing a folder move to that folder's drafts.
			const carry = next.key !== 'new' && Object.keys(carried).length > 0;
			key = next.key;
			drafts = carry ? { ...next.translations, ...carried } : next.translations;
			if (carry) {
				save('new', {});
				save();
			}
		} catch (err) {
			if (request === draftsRequest) error = err;
		} finally {
			if (request === draftsRequest) draftsLoading = false;
		}
	}

	async function fetchTranslation(code) {
		const addon = addonContext;
		pending = { ...pending, [code]: true };
		failed = without(failed, code);
		try {
			const text = await invoke('workshop_translation', { addonId: addon, language: code });
			if (addon === addonContext) loaded = { ...loaded, [code]: text };
		} catch (err) {
			if (addon === addonContext) failed = { ...failed, [code]: err };
		} finally {
			if (addon === addonContext) pending = without(pending, code);
		}
	}

	function select(code) {
		language = code;
		if (!code || drafts[code] || loaded[code] || pending[code]) return;
		if (addonId) fetchTranslation(code);
		else loaded = { ...loaded, [code]: { title: '', description: '' } };
	}

	function update(change) {
		const text = { ...current, ...change };
		if (text.title || text.description) {
			drafts = { ...drafts, [language]: text };
		} else {
			drafts = without(drafts, language);
			loaded = { ...loaded, [language]: text };
		}
		save();
	}

	export function setDescription(description) {
		update({ description });
	}

	function discard() {
		const code = language;
		drafts = without(drafts, code);
		loaded = without(loaded, code);
		save();
		select(code);
	}

	function save(target = key, translations = drafts) {
		const revision = ++saveRevision;
		pendingSaves++;
		saving = saving.then(async () => {
			try {
				await invoke('save_translations', { key: target, translations });
				if (revision === saveRevision) saveError = null;
			} catch (err) {
				if (revision === saveRevision) saveError = err;
			} finally {
				pendingSaves--;
			}
		});
	}

	export async function flush() {
		let waiting;
		do {
			waiting = saving;
			await waiting;
		} while (waiting !== saving);
		if (saveError) throw new Error(translateError(String(saveError)));
	}

	export function nameOf(code) {
		return options.find(option => option.code === code)?.name ?? code;
	}

	export function published(sent, failures) {
		const kept = new Set(failures.map(failure => failure.language));
		loaded = { ...loaded, ...Object.fromEntries(Object.entries(sent).filter(([code]) => !kept.has(code))) };
		context = null;
		syncSession(active, addonId, addonId ? null : contentPath);
	}
</script>

<div class="translations">
	<div class="language">
		<label for="translation-language">{$_('translation_language')}</label>
		<select id="translation-language" value={language} on:change={event => select(event.currentTarget.value)} disabled={disabled || draftsLoading || !!error || !key || !languages.length}>
			<option value="">{$_('translation_main')}</option>
			{#each options as option (option.code)}
				<option value={option.code}>{drafts[option.code] ? $_('translation_changed', { values: { language: option.name } }) : option.name}</option>
			{/each}
		</select>
		{#if language && drafts[language]}
			<button type="button" on:click={discard} {disabled}>{$_('translation_discard')}</button>
		{/if}
	</div>
	{#if languagesError || error}
		<div class="error" role="alert">{$_('translation_drafts_error', { values: { error: translateError(String(languagesError ?? error)) } })}{#if error}<button type="button" disabled={draftsLoading} on:click={() => loadDrafts()}>{$_('translation_retry')}</button>{/if}</div>
	{/if}
	{#if saveError}
		<div class="error" role="alert">{$_('translation_save_error', { values: { error: translateError(String(saveError)) } })}<button type="button" disabled={pendingSaves > 0} on:click={() => save()}>{$_('translation_retry')}</button></div>
	{/if}
	{#if language}
		{#if pending[language]}
			<p class="note" role="status">{$_('translation_loading', { values: { language: languageName } })}</p>
		{:else if failed[language]}
			<div class="error" role="alert">{$_('translation_load_error', { values: { language: languageName, error: translateError(String(failed[language])) } })}<button type="button" on:click={() => fetchTranslation(language)}>{$_('translation_retry')}</button></div>
		{:else if current}
			<input type="text" aria-label={$_('translation_title', { values: { language: languageName } })} placeholder={$_('translation_title', { values: { language: languageName } })} value={current.title} class:error={titleError} aria-invalid={!!titleError} on:input={event => update({ title: event.currentTarget.value })} {disabled}/>
			{#if titleError}<p class="title-error" role="alert">{translateError(titleError)}</p>{/if}
		{/if}
		<p class="note">{$_('translation_help')}</p>
	{/if}
</div>

<style>
	.translations { display: flex; flex-direction: column; gap: .5rem; flex-shrink: 0; margin-bottom: .75rem; }
	.language { display: flex; align-items: center; flex-wrap: wrap; gap: .5rem .75rem; font-size: .85em; }
	select, input, button { font: inherit; border: none; border-radius: 4px; color: var(--text); }
	select { appearance: none; min-width: 12rem; padding: .6rem .7rem; background: var(--fill); box-shadow: 0px 0px 2px 0px rgb(0 0 0 / 40%); cursor: pointer; }
	select:disabled { opacity: .5; cursor: default; }
	input { width: 100%; padding: .7rem; font-size: .85em; background: var(--fill); box-shadow: 0px 0px 2px 0px rgba(0, 0, 0, .4); }
	input:disabled { opacity: .5; }
	select:focus, input:focus { box-shadow: inset 0 0 0px 1.5px #127cff; outline: none; }
	input.error { box-shadow: inset 0 0 0px 1.5px var(--error); }
	button { padding: .5rem .7rem; background: var(--control); cursor: pointer; }
	button:disabled { opacity: .5; cursor: default; }
	button:focus-visible { outline: 2px solid #127cff; outline-offset: 1px; }
	.note, .title-error { margin: 0; font-size: .75em; line-height: 1.4; color: var(--text-muted); }
	.title-error { color: var(--text); }
	.error { padding: .7rem; background: var(--error-dark); color: #fff; border-radius: 4px; font-size: .85em; overflow-wrap: anywhere; }
	.error button { margin-inline-start: .5rem; }
</style>
