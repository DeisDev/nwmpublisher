<script>
	import { _ } from 'svelte-i18n';
	import { createEventDispatcher, onDestroy } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';

	export let id;
	export let global = false;
	export let addonId = null;
	export let contentPath = null;
	export let addonTitle = '';
	export let active = false;
	export let disabled = false;
	export let busy = false;
	export let currentChangelog = '';
	export let beforeSave = null;

	const dispatch = createEventDispatcher();
	let state = null;
	let context = null;
	let wasActive = false;
	let loadId = 0;
	let loading = false;
	let loadError = null;
	let saveError = null;
	let mode = 'blank';
	let template = '';
	let saved = false;

	$: syncSession(active, global, addonId, addonId ? null : contentPath);
	$: original = global ? state?.defaults : state?.addon;
	$: dirty = state && (mode !== (original?.mode ?? 'inherit') || template !== (original ?? state.defaults).template);
	$: locked = disabled || busy || loading || !!loadError || !state || (!global && state.key === 'new');
	$: modes = global ? ['blank', 'template', 'last_entered'] : ['inherit', 'blank', 'template', 'last_entered'];

	function syncSession(open, allAddons, addon, path) {
		const next = JSON.stringify([allAddons, addon, path]);
		if (open && (!wasActive || next !== context)) {
			context = next;
			load();
		}
		if (!open && wasActive) loadId++;
		wasActive = open;
	}

	function setPreferences(next) {
		state = next;
		const preferences = global ? next.defaults : next.addon;
		mode = preferences?.mode ?? 'inherit';
		template = (preferences ?? next.defaults).template;
	}

	async function load() {
		const request = ++loadId;
		loading = true;
		loadError = saveError = null;
		saved = false;
		try {
			const next = await invoke('get_changelog', {
				addonId: global ? null : addonId,
				contentPath: global || addonId ? null : contentPath,
			});
			if (request === loadId) setPreferences(next);
		} catch (error) {
			if (request === loadId) loadError = String(error);
		} finally {
			if (request === loadId) loading = false;
		}
	}

	async function save() {
		if (locked || !dirty) return;
		const request = loadId;
		const payload = { key: state.key, global, preferences: mode === 'inherit' ? null : { mode, template } };
		busy = true;
		saveError = null;
		saved = false;
		try {
			if (beforeSave) await beforeSave();
			const next = await invoke('save_changelog_defaults', payload);
			if (request !== loadId) return;
			setPreferences(next);
			saved = true;
			dispatch('saved', next);
		} catch (error) {
			if (request === loadId) saveError = String(error);
		} finally {
			busy = false;
		}
	}

	onDestroy(() => loadId++);
</script>

<section class="preferences" class:addon={!global} aria-labelledby={`${id}-title`}>
	<h2 id={`${id}-title`}>{$_(global ? 'changelog_defaults.global_title' : 'changelog_defaults.addon_title')}</h2>
	{#if !global && (addonTitle || contentPath)}<p class="addon-name">{addonTitle || contentPath}</p>{/if}
	<p>{$_(global ? 'changelog_defaults.global_help' : 'changelog_defaults.addon_help')}</p>
	{#if loading}<p role="status">{$_('loading')}</p>{/if}
	{#if loadError}
		<div class="error" role="alert"><p>{$_('changelog_defaults.error', { values: { error: loadError } })}</p><button type="button" disabled={disabled || loading || busy} on:click={load}>{$_('changelog_defaults.reload')}</button></div>
	{/if}
	{#if state}
		{#if !global && state.key === 'new'}<p class="notice">{$_('changelog_defaults.select_folder')}</p>{/if}
		<fieldset disabled={locked}>
			<legend>{$_('changelog_defaults.prefill')}</legend>
			{#each modes as option}
				<label class="choice" class:selected={mode === option}>
					<input type="radio" name={`${id}-mode`} value={option} bind:group={mode} on:change={() => saved = false}/>
					<span><strong>{$_('changelog_defaults.mode_' + option)}</strong><small>{option === 'inherit' ? $_('changelog_defaults.inherit_help', { values: { mode: $_('changelog_defaults.mode_' + state.defaults.mode) } }) : $_('changelog_defaults.help_' + option)}</small></span>
				</label>
			{/each}
			{#if mode === 'template'}
				<label class="template-label" for={`${id}-template`}>{$_(global ? 'changelog_defaults.global_template' : 'changelog_defaults.addon_template')}</label>
				<textarea id={`${id}-template`} rows="6" bind:value={template} on:input={() => saved = false} aria-describedby={`${id}-template-help`}></textarea>
				<p id={`${id}-template-help`}>{$_('changelog_defaults.template_help')}</p>
				{#if !global}<button type="button" on:click={() => { template = currentChangelog; saved = false; }}>{$_('changelog_defaults.copy_current')}</button>{/if}
			{/if}
		</fieldset>
		<p class="save-help">{$_('changelog_defaults.help')}</p>
		{#if saveError}<p class="error" role="alert">{$_('changelog_defaults.error', { values: { error: saveError } })}</p>{/if}
		<div class="actions"><span role="status">{#if saved}{$_(global ? 'changelog_defaults.global_saved' : 'changelog_defaults.addon_saved')}{/if}</span><button type="button" class="primary" disabled={locked || !dirty} on:click={save}>{$_(busy ? 'changelog_defaults.saving' : global ? 'changelog_defaults.save_global' : 'changelog_defaults.save_addon')}</button></div>
	{/if}
</section>

<style>
	.preferences { min-width: 0; }
	.addon { padding-bottom: 1.5rem; margin-bottom: 1.5rem; border-bottom: 1px solid #414141; }
	h2 { margin: 0; font-size: 1em; }
	p { color: #aaa; line-height: 1.5; font-size: .85em; margin: .5rem 0 1rem; }
	.addon-name { color: #fff; font-weight: 600; overflow-wrap: anywhere; margin-bottom: .5rem; }
	fieldset { border: 0; margin: 0; padding: 0; min-width: 0; }
	legend, .template-label { font-size: .9em; font-weight: 600; padding: 0; margin-bottom: .6rem; }
	.choice { display: flex; align-items: flex-start; gap: .6rem; padding: .65rem; margin-bottom: .5rem; border: 1px solid #414141; border-radius: 4px; cursor: pointer; }
	.addon .choice { gap: .75rem; padding: 1rem; margin-bottom: .75rem; }
	.choice.selected { background: #292929; border-color: #aaa; }
	.choice span { min-width: 0; }
	.choice strong { font-size: .85em; font-weight: 600; }
	.choice small { display: block; color: #aaa; font-size: .8em; line-height: 1.4; margin-top: .25rem; }
	input { margin: .15rem 0 0; accent-color: var(--neutral); flex-shrink: 0; }
	fieldset:disabled { opacity: .6; }
	fieldset:disabled .choice { cursor: default; }
	.template-label { display: block; margin-top: 1rem; }
	button, textarea { font: inherit; color: #fff; border: 0; border-radius: 4px; padding: .65rem; font-size: .85em; }
	textarea { width: 100%; box-sizing: border-box; background: rgba(255,255,255,.1); min-height: 7rem; resize: vertical; }
	button { background: #313131; cursor: pointer; }
	.addon button { padding: .8rem 1rem; }
	button:hover:enabled { background: #414141; }
	button:disabled { opacity: .5; cursor: default; }
	button:focus-visible, textarea:focus-visible, input:focus-visible { outline: 2px solid #127cff; outline-offset: 2px; }
	.actions { display: flex; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: .75rem; }
	.addon .actions { margin-top: 1.25rem; }
	.actions span { margin-right: auto; font-size: .8em; color: #aaa; }
	.primary { background: var(--neutral); }
	.primary:hover:enabled { background: var(--neutral-dark); }
	.save-help { margin-top: 1rem; }
	.error { padding: .7rem; background: var(--error-dark); color: #fff; border-radius: 4px; overflow-wrap: anywhere; }
	.error p { color: inherit; }
	.notice { padding: .75rem; background: #292929; border-radius: 4px; }
</style>
