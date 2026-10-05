<script>
	import { _ } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import * as dialog from '@tauri-apps/plugin-dialog';
	import { changelogImportSection, changelogToBBCode, markdownToBBCode } from '../bbcode';
	import { readAddonDocument } from '../addon-documents';
	import BBCodeEditor from './BBCodeEditor.svelte';

	export let addonId = null;
	export let contentPath = null;
	export let active = false;
	export let editorActive = false;
	export let disabled = false;
	export let value = '';
	export let busy = false;
	export let invalid = null;

	let state = null;
	let context = null;
	let wasActive = false;
	let loadId = 0;
	let loading = false;
	let pendingDrafts = 0;
	let draftSave = Promise.resolve();
	let draftRevision = 0;
	let draftError = null;
	let error = null;
	let touched = false;
	let historyKey = 0;
	let formattingOpen = false;
	let editor;
	let importing = false;
	const changelogMaxBytes = 7999;
	const encoder = new TextEncoder();

	$: syncSession(active, addonId, addonId ? null : contentPath);
	$: bytes = encoder.encode(value).length;
	$: invalid = bytes > changelogMaxBytes ? 'ERR_CHANGELOG_TOO_LONG' : value.includes('\0') ? 'ERR_CHANGELOG_CONTAINS_NUL' : null;
	$: busy = loading || !!error || !!draftError || !!invalid;

	function syncSession(open, id, path) {
		const next = JSON.stringify([id, path]);
		if (open && (!wasActive || next !== context)) {
			const keepText = wasActive && touched;
			context = next;
			load(id, path, keepText);
		}
		if (!open && wasActive) loadId++;
		wasActive = open;
	}

	async function load(id = addonId, path = addonId ? null : contentPath, keepText = false) {
		const request = ++loadId;
		loading = true;
		error = null;
		try {
			// Finish the previous editor's writes before requesting its saved text again.
			await flush();
			const next = await invoke('get_changelog', { addonId: id, contentPath: path });
			if (request !== loadId) return;
			const preserveText = keepText && (state?.key === next.key || (state?.key === 'new' && !id));
			state = next;
			draftError = null;
			if (!preserveText) {
				value = next.text;
				touched = false;
			}
			historyKey++;
			formattingOpen = false;
			if (preserveText) remember();
		} catch (err) {
			if (request === loadId) error = String(err);
		} finally {
			if (request === loadId) loading = false;
		}
	}

	function remember() {
		if (state?.mode !== 'last_entered') return;
		const key = state.key;
		const text = value;
		const revision = ++draftRevision;
		pendingDrafts++;
		draftSave = draftSave.then(async () => {
			try {
				await invoke('remember_changelog', { key, text });
				if (revision === draftRevision) draftError = null;
			} catch (err) {
				if (revision === draftRevision) draftError = String(err);
			} finally {
				pendingDrafts--;
			}
		});
	}

	function onInput(event) {
		value = event.detail;
		touched = true;
		remember();
	}

	function convertPaste(text) {
		return AppSettings.bbcode_convert_pasted_changelogs ? changelogToBBCode(text) : null;
	}

	async function importChangelog() {
		importing = true;
		try {
			const text = await readAddonDocument(contentPath, 'changelog', 'CHANGELOG.md');
			if (text === null) return;
			const section = changelogImportSection(text);
			if (section) editor.insert(markdownToBBCode(section));
			else await dialog.message($_('addon_document.changelog_empty'), { kind: 'warning' });
		} finally {
			importing = false;
		}
	}

	export async function flush() {
		let pending;
		do {
			pending = draftSave;
			await pending;
		} while (pending !== draftSave);
		if (draftError) throw new Error(draftError);
	}

	export function applyDefaults(next) {
		if (state?.key !== next.key) return;
		state = next;
		if (!touched) value = next.text;
		else remember();
	}
</script>

<div class="changelog-editor">
	{#if error}
		<div class="error" role="alert">{$_('changelog_defaults.error', { values: { error } })}<button type="button" disabled={loading} on:click={() => load(addonId, addonId ? null : contentPath, touched)}>{$_('changelog_defaults.reload')}</button></div>
	{/if}
	{#if draftError}
		<div class="error" role="alert">{$_('changelog_defaults.draft_error', { values: { error: draftError } })}<button type="button" disabled={pendingDrafts > 0} on:click={remember}>{$_('changelog_defaults.retry')}</button></div>
	{/if}
	<BBCodeEditor bind:this={editor} id="changes" label={$_('changelog_optional')} {value} on:input={onInput} disabled={disabled || loading || !!error || !state} error={invalid} size={$_('workshop_description_size', { values: { bytes, max: changelogMaxBytes } })} bind:formattingOpen active={active && editorActive} {historyKey} {convertPaste} importLabel={$_('addon_document.import', { values: { file: 'CHANGELOG.md' } })} importDisabled={!contentPath || importing} on:import={importChangelog}/>
	{#if state?.mode === 'last_entered'}<p class="draft-status" role="status">{$_(draftError ? 'changelog_defaults.unsaved_draft' : 'changelog_defaults.remember_help')}</p>{/if}
</div>

<style>
	.changelog-editor { display: flex; flex-direction: column; gap: .5rem; flex: 1; min-height: 0; min-width: 0; }
	.error { flex-shrink: 0; padding: .7rem; background: var(--error-dark); color: #fff; border-radius: 4px; font-size: .85em; overflow-wrap: anywhere; }
	.error button { font: inherit; padding: .5rem; margin-left: .5rem; border: 0; border-radius: 4px; background: var(--control); color: var(--text); cursor: pointer; }
	.error button:disabled { opacity: .5; cursor: default; }
	.error button:focus-visible { outline: 2px solid #127cff; outline-offset: 1px; }
	.draft-status { font-size: .75em; line-height: 1.4; color: var(--text-muted); margin: 0; flex-shrink: 0; }
</style>
