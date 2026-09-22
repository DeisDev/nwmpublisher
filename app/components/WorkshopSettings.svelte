<script>
	import { _, locale } from 'svelte-i18n';
	import { createEventDispatcher, onDestroy } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import Search from '@lucide/svelte/icons/search';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Loading from './Loading.svelte';

	export let item;
	export let active = false;
	export let disabled = false;
	export let busy = false;
	export let dirty = false;
	const dispatch = createEventDispatcher();
	const visibilityOptions = ['public', 'friends_only', 'private', 'unlisted'];
	let sessionId = null;
	let wasActive = false;
	let loadId = 0;
	let searchId = 0;
	let loading = true;
	let loaded = false;
	let canEdit = false;
	let original = [];
	let dependencies = [];
	let originalVisibility = null;
	let visibility = null;
	let error = null;
	let needsReload = false;
	let saved = false;
	let query = '';
	let searchedQuery = '';
	let results = [];
	let total = 0;
	let page = 0;
	let searching = false;
	let searchError = null;
	let searched = false;
	let announcement = '';

	$: syncSession(active, item.id);
	$: additions = dependencies.filter(entry => !original.some(old => old.id === entry.id));
	$: removals = original.filter(entry => !dependencies.some(current => current.id === entry.id));
	$: dirty = loaded && (additions.length > 0 || removals.length > 0 || visibility !== originalVisibility);
	$: locked = disabled || busy || loading || !loaded || !canEdit;
	$: numberLocale = $locale === 'kr' ? 'ko' : ($locale ?? 'en');

	function syncSession(open, id) {
		if (open && (!wasActive || sessionId !== String(id))) {
			sessionId = String(id);
			loaded = false;
			query = '';
			results = [];
			searched = false;
			searchError = null;
			searching = false;
			searchId++;
			saved = false;
			load(false);
		}
		wasActive = open;
	}

	async function load(preserveDraft) {
		const requestId = ++loadId;
		const id = sessionId;
		loading = true;
		error = null;
		try {
			const details = await invoke('workshop_details', { addonId: id });
			if (requestId !== loadId) return;
			const pendingAdditions = dependencies.filter(entry => !original.some(old => old.id === entry.id));
			const pendingRemovals = original.filter(entry => !dependencies.some(current => current.id === entry.id));
			const visibilityChanged = visibility !== originalVisibility;
			original = details.dependencies;
			originalVisibility = details.item.visibility;
			if (!preserveDraft || !loaded) {
				dependencies = [...original];
				visibility = originalVisibility;
			} else {
				dependencies = [
					...original.filter(entry => !pendingRemovals.some(removed => removed.id === entry.id)),
					...pendingAdditions.filter(entry => !original.some(current => current.id === entry.id)),
				];
				if (!visibilityChanged) visibility = originalVisibility;
			}
			canEdit = details.canEdit;
			loaded = true;
			needsReload = false;
			dispatch('details', details.item);
		} catch (err) {
			if (requestId === loadId) {
				error = String(err);
				needsReload = true;
			}
		} finally {
			if (requestId === loadId) loading = false;
		}
	}

	export function refresh() { return load(true); }

	function discard() {
		dependencies = [...original];
		visibility = originalVisibility;
		saved = false;
	}

	function add(entry) {
		if (locked || entry.banned || entry.id === sessionId || dependencies.some(current => current.id === entry.id)) return;
		dependencies = [...dependencies, entry];
		saved = false;
		announcement = $_('workshop_dependency_added', { values: { title: entry.title ?? entry.id } });
	}

	function remove(entry) {
		dependencies = dependencies.filter(current => current.id !== entry.id);
		saved = false;
		announcement = $_('workshop_dependency_removed', { values: { title: entry.title ?? entry.id } });
	}

	function onSearchInput() {
		searchId++;
		searching = false;
		searchError = null;
		searched = false;
		results = [];
	}

	async function search(more = false) {
		if (locked || searching || !query.trim()) return;
		const requestId = ++searchId;
		const term = more ? searchedQuery : query.trim();
		const nextPage = more ? page + 1 : 1;
		searching = true;
		searchError = null;
		if (!more) { results = []; searched = false; }
		try {
			const response = await invoke('search_required_addons', { query: term, page: nextPage });
			if (requestId !== searchId) return;
			results = more ? [...results, ...response.items.filter(entry => !results.some(current => current.id === entry.id))] : response.items;
			total = response.total;
			page = nextPage;
			searchedQuery = term;
			searched = true;
		} catch (err) {
			if (requestId === searchId) searchError = String(err);
		} finally {
			if (requestId === searchId) searching = false;
		}
	}

	async function save() {
		if (locked || !dirty || needsReload) return;
		busy = true;
		error = null;
		saved = false;
		try {
			const result = await invoke('save_workshop_settings', { request: {
				addonId: sessionId,
				originalDependencies: original.map(entry => entry.id),
				dependencies: dependencies.map(entry => entry.id),
				originalVisibility,
				visibility,
			} });
			const known = [...dependencies, ...original];
			original = result.dependencies.map(id => known.find(entry => entry.id === id) ?? { id, title: null, previewUrl: null, banned: null });
			originalVisibility = result.visibility;
			dispatch('saved');
			if (result.error) {
				error = result.error;
				needsReload = true;
			} else {
				dependencies = [...original];
				visibility = originalVisibility;
				saved = true;
				await load(true);
			}
		} catch (err) {
			error = String(err);
			needsReload = true;
		} finally {
			busy = false;
		}
	}

	onDestroy(() => { loadId++; searchId++; });
</script>

<div class="settings">
	<slot></slot>
	<header>
		<div><h2>{$_('workshop_settings')}</h2><p>{$_('workshop_settings_help')}</p></div>
		<button type="button" class="refresh" disabled={loading || busy || disabled} on:click={() => load(true)} aria-label={$_('workshop_reload')} title={$_('workshop_reload')}><RefreshCw size="1rem"/></button>
	</header>
	{#if loading}<p class="notice" role="status"><Loading inline size="1rem"/> {$_('workshop_details_loading')}</p>{/if}
	{#if error}
		<div class="error" role="alert"><p>{error}</p><button type="button" disabled={loading || busy || disabled} on:click={() => load(true)}>{$_('workshop_reload_review')}</button></div>
	{/if}
	{#if loaded && !canEdit}<p class="notice">{$_('workshop_owner_only')}</p>{/if}
	{#if saved}<p class="success" role="status">{$_('workshop_settings_saved')}</p>{/if}
	{#if loaded}
		<section class="visibility">
			<label for="workshop-visibility">{$_('workshop_settings_visibility')}</label>
			<select id="workshop-visibility" bind:value={visibility} disabled={locked} on:change={() => saved = false} aria-describedby="visibility-help">
				{#each visibilityOptions as option}<option value={option}>{$_('workshop_visibility_' + option)}</option>{/each}
			</select>
			<p id="visibility-help">{$_('workshop_visibility_help_' + visibility)}</p>
		</section>
		<section class="dependencies">
			<h3>{$_('workshop_required_addons')} <span class="count">{dependencies.length}</span></h3>
			<p>{$_('workshop_dependencies_help')}</p>
			{#if dependencies.length === 0}<div class="empty">{$_('workshop_no_dependencies')}</div>{/if}
			<ul class="addon-list">
				{#each dependencies as entry (entry.id)}
					<li class:pending={additions.some(added => added.id === entry.id)}>
						{#if entry.previewUrl}<img src={entry.previewUrl} alt="" loading="lazy"/>{:else}<div class="image-placeholder" aria-hidden="true">?</div>{/if}
						<div class="addon-text"><a href={`https://steamcommunity.com/sharedfiles/filedetails/?id=${entry.id}`} target="_blank" rel="noreferrer">{entry.title ?? $_('workshop_unavailable_addon')}<ExternalLink size=".75rem"/></a><small>{entry.id}{#if entry.banned} · {$_('workshop_banned')}{:else if !entry.title} · {$_('workshop_dependency_unavailable')}{/if}{#if additions.some(added => added.id === entry.id)} · <span class="pending-label">{$_('workshop_pending_addition')}</span>{/if}</small></div>
						<button type="button" class="icon-button" disabled={locked} on:click={() => remove(entry)} aria-label={$_('workshop_remove_dependency', { values: { title: entry.title ?? entry.id } })}><X size="1rem"/></button>
					</li>
				{/each}
			</ul>
			{#if removals.length}
				<div class="removed"><span>{$_('workshop_pending_removals')}</span>{#each removals as entry (entry.id)}<button type="button" disabled={locked} on:click={() => { dependencies = [...dependencies, entry]; saved = false; }}>{entry.title ?? entry.id} · {$_('workshop_undo')}</button>{/each}</div>
			{/if}
			<form on:submit|preventDefault={() => search()}>
				<label for="dependency-search">{$_('workshop_find_addons')}</label>
				<div class="search-input"><input id="dependency-search" type="search" bind:value={query} on:input={onSearchInput} disabled={locked} maxlength="512" placeholder={$_('workshop_search_placeholder')}/><button type="submit" disabled={locked || searching || !query.trim()}><Search size="1rem"/>{$_('workshop_search')}</button></div>
				<p>{$_('workshop_search_help')}</p>
			</form>
			{#if searchError}<p class="error" role="alert">{searchError}</p>{/if}
			{#if searched}
				<p class="result-count" role="status">{results.length ? $_('workshop_search_results', { values: { count: total.toLocaleString(numberLocale) } }) : $_('workshop_search_empty')}</p>
				<ul class="addon-list search-results">
					{#each results as entry (entry.id)}
						{@const selected = dependencies.some(current => current.id === entry.id)}
						{@const self = entry.id === sessionId}
						<li>
							{#if entry.previewUrl}<img src={entry.previewUrl} alt="" loading="lazy"/>{:else}<div class="image-placeholder" aria-hidden="true">?</div>{/if}
							<div class="addon-text"><a href={`https://steamcommunity.com/sharedfiles/filedetails/?id=${entry.id}`} target="_blank" rel="noreferrer">{entry.title}<ExternalLink size=".75rem"/></a><small>{entry.id}{#if entry.banned} · {$_('workshop_banned')}{/if}</small></div>
							<button type="button" disabled={locked || selected || self || entry.banned} on:click={() => add(entry)} aria-label={$_('workshop_add_dependency', { values: { title: entry.title ?? entry.id } })}>{#if selected}{$_('workshop_added')}{:else if self}{$_('workshop_this_addon')}{:else}<Plus size=".85rem"/>{$_('workshop_add')}{/if}</button>
						</li>
					{/each}
				</ul>
				{#if page * 50 < total}<button type="button" disabled={locked || searching} on:click={() => search(true)}>{$_('workshop_load_more')}</button>{/if}
			{/if}
			{#if searching}<p class="notice" role="status"><Loading inline size="1rem"/> {$_('workshop_searching')}</p>{/if}
		</section>
	{/if}
</div>
<footer>
	<div class="save-status" aria-live="polite">{#if dirty}{$_('workshop_pending_summary', { values: { added: additions.length, removed: removals.length } })}{#if visibility !== originalVisibility} · {$_('workshop_visibility_changed')}{/if}{:else}{$_('workshop_no_pending_changes')}{/if}</div>
	<div class="save-actions"><button type="button" disabled={locked || !dirty} on:click={discard}>{$_('workshop_discard')}</button><button type="button" class="primary" disabled={locked || !dirty || needsReload} on:click={save}>{#if busy}<Loading inline size="1rem"/>{/if}{$_(busy ? 'workshop_saving' : 'workshop_save_settings')}</button></div>
</footer>
<span class="sr-only" role="status">{announcement}</span>

<style>
	.settings { overflow: auto; flex: 1; min-height: 0; padding: 1rem 1rem 1.5rem; }
	header { display: flex; align-items: flex-start; justify-content: space-between; gap: 1rem; }
	h2 { margin: 0; font-size: 1em; }
	h3 { margin: 0 0 .5rem; font-size: .9em; }
	p { margin: .5rem 0 1rem; color: #aaa; line-height: 1.5; font-size: .85em; }
	button, select, input { font: inherit; font-size: .85em; border: 0; border-radius: 4px; color: #fff; padding: .8rem 1rem; }
	button { display: inline-flex; align-items: center; justify-content: center; gap: .5rem; cursor: pointer; flex-shrink: 0; background: #313131; line-height: 1.2; transition: background-color .1s; }
	button:hover:enabled { background: #414141; }
	button:disabled { background: #313131; color: #aaa; cursor: default; }
	button:focus-visible, a:focus-visible { outline: 2px solid #127cff; outline-offset: 1px; }
	input, select { background: rgba(255,255,255,.1); box-shadow: 0 0 2px rgba(0,0,0,.4); }
	input:focus, select:focus { outline: none; box-shadow: inset 0 0 0 1.5px #127cff; }
	input:disabled, select:disabled { color: #aaa; cursor: default; }
	.refresh, .icon-button { padding: .8rem; }
	.visibility { border-bottom: 1px solid #414141; padding-bottom: 1.5rem; margin: 1.5rem 0; }
	label { display: block; font-size: .9em; font-weight: 600; margin-bottom: .75rem; }
	select { width: 100%; cursor: pointer; }
	option { background: #313131; color: #fff; }
	.visibility p { margin-bottom: 0; }
	.count { color: #aaa; background: #313131; border-radius: 4px; padding: .1rem .4rem; font-size: .85em; margin-left: .3rem; }
	.addon-list { list-style: none; margin: 1rem 0; padding: 0; }
	li { display: flex; align-items: center; gap: 1rem; padding: 1rem; border-left: 3px solid transparent; background: #292929; border-radius: 4px; margin-bottom: .75rem; }
	li.pending { border-left-color: var(--neutral); }
	li img, .image-placeholder { width: 2.6rem; height: 2.6rem; border-radius: 4px; flex-shrink: 0; object-fit: cover; background: #313131; }
	.image-placeholder { display: grid; place-items: center; color: #aaa; }
	.addon-text { min-width: 0; flex: 1; }
	a { display: flex; align-items: center; gap: .3rem; color: #46b0ff; overflow-wrap: anywhere; font-size: .9em; text-decoration: none; }
	a:hover { text-decoration: underline; }
	small { display: block; color: #aaa; font-size: .8em; margin-top: .25rem; overflow-wrap: anywhere; }
	.pending-label { color: #fff; }
	.empty { border-radius: 4px; padding: 1.25rem; background: #292929; color: #aaa; text-align: center; font-size: .85em; }
	.removed { display: flex; flex-wrap: wrap; align-items: center; gap: .75rem; padding: .75rem 0; color: #aaa; font-size: .85em; }
	.removed button { font-size: inherit; padding: .5rem .75rem; }
	form { margin-top: 1.5rem; padding-top: 1.5rem; border-top: 1px solid #414141; }
	.search-input { display: flex; gap: .75rem; }
	input { min-width: 0; flex: 1; }
	.search-results { margin-top: 1rem; }
	.error { background: var(--error-dark); color: #fff; padding: 1rem; border-radius: 4px; overflow-wrap: anywhere; }
	.error p { color: inherit; }
	.success { background: var(--success-dark); color: #fff; padding: 1rem; border-radius: 4px; }
	.notice { display: flex; gap: .5rem; align-items: center; }
	footer { flex-shrink: 0; border-top: 1px solid #414141; padding: 1rem; }
	.save-status { color: #aaa; font-size: .8em; margin-bottom: .75rem; }
	.save-actions { display: flex; justify-content: flex-end; gap: .75rem; flex-wrap: wrap; }
	.primary { background: var(--neutral); text-shadow: 0 1px rgba(0,0,0,.6); }
	.primary:hover:enabled { background: var(--neutral-dark); }
	.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
</style>
