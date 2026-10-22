<script>
	import { _, locale } from 'svelte-i18n';
	import { Channel, invoke } from '@tauri-apps/api/core';
	import { get } from 'svelte/store';
	import Filter from '@lucide/svelte/icons/list-filter';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import SquareCheck from '@lucide/svelte/icons/square-check';
	import Check from '@lucide/svelte/icons/check';
	import Plus from '@lucide/svelte/icons/plus';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import GitCompare from '@lucide/svelte/icons/git-compare';
	import ListMinus from '@lucide/svelte/icons/list-minus';
	import Search from '@lucide/svelte/icons/search';
	import Loading from '../components/Loading.svelte';
	import Modal from '../components/Modal.svelte';
	import Timestamp from '../components/Timestamp.svelte';
	import CollectionEditor from '../components/CollectionEditor.svelte';
	import CollectionCompare from '../components/CollectionCompare.svelte';
	import { collections, compareTitles, ensureCollections, loadCollections } from '../collections.js';
	import { saveSettings } from '../settings.js';
	import { taskMessage } from '../transactions.js';

	const visibilityOptions = ['public', 'friends_only', 'private', 'unlisted'];
	const sortOptions = ['updated_desc', 'updated_asc', 'created_desc', 'created_asc', 'title_asc', 'title_desc', 'items_desc', 'items_asc', 'subscribers_desc', 'subscribers_asc'];
	let visibility = [...AppSettings.collections_visibility];
	let sort = AppSettings.collections_sort;
	let saved = { visibility, sort };
	let saveRequest = 0;
	let saveError = null;
	let filtersOpen = false;
	let filterMenu;
	let filterButton;
	let query = '';
	let view = null;
	let notice = null;
	let listNotice = null;
	let selecting = false;
	let selected = new Set();
	let actionBusy = false;
	let actionErrors = [];

	ensureCollections();

	$: all = $collections.items;
	$: byId = new Map(all.map(collection => [collection.id, collection]));
	$: visible = selectCollections(all, visibility, sort, query);
	$: selectedList = [...selected].filter(id => byId.has(id));
	$: numberLocale = $locale === 'kr' ? 'ko' : ($locale ?? 'en');
	$: if (view?.kind === 'edit' && $collections.loaded && !$collections.loading && !byId.has(view.id)) view = null;
	$: if (view?.kind === 'compare' && !view.ids.every(id => byId.has(id))) view = null;

	function selectCollections(items, selectedVisibility, selectedSort, search) {
		const [field, direction] = selectedSort.split('_');
		const multiplier = direction === 'asc' ? 1 : -1;
		const term = search.trim().toLocaleLowerCase();
		return items.filter(collection => selectedVisibility.includes(collection.fields.visibility) && (!term || collection.id === term || collection.fields.title.toLocaleLowerCase().includes(term))).sort((a, b) => {
			let order;
			if (field === 'title') order = compareTitles(a.fields.title, b.fields.title);
			else if (field === 'items') order = a.fields.items.length - b.fields.items.length;
			else {
				const property = { updated: 'timeUpdated', created: 'timeCreated', subscribers: 'subscriptions' }[field];
				order = a[property] - b[property];
			}
			return order * multiplier || a.id.length - b.id.length || a.id.localeCompare(b.id);
		});
	}

	function savePreferences(nextVisibility, nextSort) {
		const request = ++saveRequest;
		visibility = nextVisibility;
		sort = nextSort;
		saveError = null;
		saveSettings({
			collections_visibility: nextVisibility,
			collections_sort: nextSort,
		}).then(() => {
			saved = { visibility: nextVisibility, sort: nextSort };
			AppSettings.collections_visibility = nextVisibility;
			AppSettings.collections_sort = nextSort;
		}, error => {
			saveError = String(error);
			if (request !== saveRequest) return;
			visibility = saved.visibility;
			sort = saved.sort;
		});
	}

	function toggleVisibility(value, checked) {
		savePreferences(visibilityOptions.filter(option => option === value ? checked : visibility.includes(option)), sort);
	}

	function dismissFilters(event) {
		if (filtersOpen && !filterMenu?.contains(event.target)) filtersOpen = false;
	}

	function onKeydown(event) {
		if (filtersOpen && event.key === 'Escape') {
			filtersOpen = false;
			filterButton.focus();
		}
	}

	function toggleSelecting() {
		selecting = !selecting;
		selected = new Set();
	}

	function toggleSelected(id, checked = !selected.has(id)) {
		const next = new Set(selected);
		if (checked) next.add(id);
		else next.delete(id);
		selected = next;
	}

	function openCollection(collection) {
		if (selecting) {
			toggleSelected(collection.id);
			return;
		}
		notice = null;
		listNotice = null;
		view = { kind: 'edit', id: collection.id };
	}

	function newCollection() {
		notice = null;
		listNotice = null;
		view = { kind: 'new' };
	}

	async function created({ detail }) {
		await loadCollections();
		if (get(collections).items.some(collection => collection.id === detail.id)) {
			notice = detail.error;
			view = { kind: 'edit', id: detail.id };
		} else {
			view = null;
			listNotice = detail.error ?? $_('collections_created_pending');
		}
	}

	function failureList(outcomes) {
		return outcomes.filter(outcome => outcome.error).map(outcome => ({ title: byId.get(outcome.id)?.fields.title ?? outcome.id, error: outcome.error }));
	}

	async function changeVisibility(value) {
		const ids = selectedList;
		if (!value || !ids.length || actionBusy) return;
		actionBusy = true;
		actionErrors = [];
		try {
			const outcomes = await invoke('set_collections_visibility', { collections: ids, visibility: value });
			actionErrors = failureList(outcomes);
			const changed = outcomes.length - actionErrors.length;
			if (changed) taskMessage($_('collections_visibility_changed', { values: { count: changed } }));
		} catch (error) {
			actionErrors = [{ title: $_('collections'), error: String(error) }];
		} finally {
			actionBusy = false;
			loadCollections();
		}
	}

	let deleting = null;

	function confirmDelete(ids) {
		actionErrors = [];
		deleting = { ids, busy: false };
	}

	async function deleteCollections() {
		const ids = deleting.ids;
		deleting = { ...deleting, busy: true };
		try {
			const outcomes = await invoke('delete_collections', { collections: ids });
			actionErrors = failureList(outcomes);
			const removed = outcomes.filter(outcome => !outcome.error).map(outcome => outcome.id);
			if (removed.length) taskMessage($_('collections_deleted', { values: { count: removed.length } }));
			selected = new Set([...selected].filter(id => !removed.includes(id)));
			if (view?.kind === 'edit' && removed.includes(view.id)) view = null;
		} catch (error) {
			actionErrors = [{ title: $_('collections'), error: String(error) }];
		} finally {
			deleting = null;
			loadCollections();
		}
	}

	let removing = null;

	function openRemove(reference = '') {
		removing = { reference, entry: null, targets: new Set(), looking: false, busy: false, progress: null, error: null, failures: [] };
		if (reference) findRemoval();
	}

	async function findRemoval() {
		const reference = removing.reference.trim();
		if (!reference) return;
		removing = { ...removing, looking: true, error: null, entry: null };
		try {
			const lookup = await invoke('collection_entries', { references: [reference] });
			const entry = lookup.entries[0];
			if (!removing) return;
			if (!entry) removing = { ...removing, error: $_('collections_remove_invalid') };
			else removing = { ...removing, entry, targets: new Set(all.filter(collection => collection.fields.items.includes(entry.id)).map(collection => collection.id)) };
		} catch (error) {
			if (removing) removing = { ...removing, error: String(error) };
		} finally {
			if (removing) removing = { ...removing, looking: false };
		}
	}

	function toggleTarget(id, checked) {
		const targets = new Set(removing.targets);
		if (checked) targets.add(id);
		else targets.delete(id);
		removing = { ...removing, targets };
	}

	async function removeEverywhere() {
		const { entry, targets } = removing;
		removing = { ...removing, busy: true, error: null, failures: [], progress: { completed: 0, total: targets.size } };
		try {
			const outcomes = await invoke('change_collection_items', {
				collections: [...targets],
				items: [entry.id],
				add: false,
				onProgress: new Channel(progress => { if (removing) removing = { ...removing, progress }; }),
			});
			const failures = failureList(outcomes);
			const changed = outcomes.filter(outcome => outcome.changed.length).length;
			if (changed) taskMessage($_('collections_removed_everywhere', { values: { count: changed } }));
			removing = failures.length ? { ...removing, failures, busy: false, progress: null } : null;
		} catch (error) {
			removing = { ...removing, error: String(error), busy: false, progress: null };
		} finally {
			loadCollections();
		}
	}

	function cancelRemove() {
		if (!removing?.busy) removing = null;
	}

	$: removalChoices = removing?.entry ? all.filter(collection => collection.fields.items.includes(removing.entry.id)) : [];
</script>

<svelte:window on:pointerdown={dismissFilters} on:keydown={onKeydown}/>

{#if view?.kind === 'edit' && byId.has(view.id)}
	{#key view.id}
		<CollectionEditor collection={byId.get(view.id)} others={all.filter(collection => collection.id !== view.id)} {notice} on:close={() => view = null} on:saved={loadCollections} on:delete={event => confirmDelete([event.detail])} on:removeEverywhere={event => openRemove(event.detail)}/>
	{/key}
{:else if view?.kind === 'new'}
	<CollectionEditor on:close={() => view = null} on:created={created}/>
{:else if view?.kind === 'compare'}
	<CollectionCompare left={byId.get(view.ids[0])} right={byId.get(view.ids[1])} on:close={() => view = null}/>
{:else}
	<section class="collections">
		<div class="toolbar">
			<div class="filters" bind:this={filterMenu}>
				<button type="button" class:active={visibility.length !== visibilityOptions.length} aria-expanded={filtersOpen} aria-controls="collections-filters" bind:this={filterButton} on:click={() => filtersOpen = !filtersOpen}>
					<Filter size="1rem"/>
					{$_('workshop_filters')} ({visibility.length}/{visibilityOptions.length})
					<ChevronDown size="1rem"/>
				</button>
				{#if filtersOpen}
					<fieldset id="collections-filters">
						<legend>{$_('workshop_visibility')}</legend>
						{#each visibilityOptions as value}
							<label>
								<input type="checkbox" checked={visibility.includes(value)} on:change={event => toggleVisibility(value, event.currentTarget.checked)}/>
								{$_('workshop_visibility_' + value)}
							</label>
						{/each}
					</fieldset>
				{/if}
			</div>
			<label class="sort">
				<span>{$_('workshop_sort')}</span>
				<select value={sort} on:change={event => savePreferences(visibility, event.currentTarget.value)}>
					{#each sortOptions as value}
						<option {value}>{$_('collections_sort_' + value)}</option>
					{/each}
				</select>
			</label>
			<div class="search"><Search size="1rem"/><input type="search" bind:value={query} placeholder={$_('collections_search')} aria-label={$_('collections_search')}/></div>
			<span class="spacer"></span>
			<button type="button" disabled={!$collections.loaded} on:click={() => openRemove()}><ListMinus size="1rem"/>{$_('collections_remove_everywhere')}</button>
			<button type="button" class:active={selecting} disabled={!all.length} on:click={toggleSelecting}>
				{#if selecting}<Check size="1rem"/>{$_('done')}{:else}<SquareCheck size="1rem"/>{$_('select')}{/if}
			</button>
			<button type="button" class="primary" on:click={newCollection}><Plus size="1rem"/>{$_('collections_new')}</button>
			<button type="button" class="icon-button" disabled={$collections.loading} on:click={loadCollections} aria-label={$_('collections_reload')} title={$_('collections_reload')}><RefreshCw size="1rem"/></button>
		</div>

		{#if selecting}
			<div class="panel">
				<div class="selection-summary">
					<span class="count">{$_('collections_selected', { values: { count: selectedList.length } })}</span>
					<button type="button" class="plain" on:click={() => selected = new Set(visible.map(collection => collection.id))}>{$_('collections_select_shown')}</button>
					<button type="button" class="plain" disabled={!selectedList.length} on:click={() => selected = new Set()}>{$_('clear_selection')}</button>
				</div>
				<div class="selection-actions">
					<select value="" disabled={!selectedList.length || actionBusy} on:change={event => { const value = event.currentTarget.value; event.currentTarget.value = ''; changeVisibility(value); }} aria-label={$_('collections_change_visibility')}>
						<option value="" disabled>{$_('collections_change_visibility')}</option>
						{#each visibilityOptions as option}<option value={option}>{$_('workshop_visibility_' + option)}</option>{/each}
					</select>
					<button type="button" disabled={selectedList.length !== 2 || actionBusy} title={$_('collections_compare_help')} on:click={() => view = { kind: 'compare', ids: selectedList }}><GitCompare size="1rem"/>{$_('collections_compare')}</button>
					<button type="button" disabled={!selectedList.length || actionBusy} on:click={() => confirmDelete(selectedList)}><Trash2 size="1rem"/>{$_('collections_delete', { values: { count: selectedList.length } })}</button>
					{#if actionBusy}<Loading inline size="1rem"/>{/if}
				</div>
			</div>
		{/if}

		{#if saveError}<p class="error" role="alert">{$_('workshop_save_error', { values: { error: saveError } })}</p>{/if}
		{#if listNotice}<p class="notice" role="status">{listNotice}</p>{/if}
		{#if actionErrors.length}
			<ul class="error failures" role="alert">
				{#each actionErrors as failure}<li><strong>{failure.title}</strong>: {failure.error}</li>{/each}
			</ul>
		{/if}

		{#if !$collections.loaded && $collections.loading}
			<p class="status" role="status"><Loading inline size="1rem"/> {$_('collections_loading')}</p>
		{:else if $collections.error}
			<div class="error" role="alert">
				<p>{$_('collections_load_error', { values: { error: $collections.error } })}</p>
				<button type="button" on:click={loadCollections}>{$_('workshop_retry')}</button>
			</div>
		{:else if !all.length}
			<p class="status">{$_('collections_none')}</p>
		{:else}
			<p class="status" role="status">
				{#if visibility.length === 0}
					{$_('collections_no_visibility')}
				{:else if !visible.length}
					{$_('collections_no_matches')}
				{:else}
					{$_('collections_showing', { values: { num: visible.length.toLocaleString(numberLocale), total: all.length.toLocaleString(numberLocale) } })}
				{/if}
			</p>
		{/if}

		<ul class="list scrollbar">
			{#each visible as collection (collection.id)}
				<li class:selected={selected.has(collection.id)}>
					{#if selecting}<input type="checkbox" checked={selected.has(collection.id)} on:change={event => toggleSelected(collection.id, event.currentTarget.checked)} aria-label={$_('collections_select_item', { values: { title: collection.fields.title } })}/>{/if}
					<button type="button" class="row" on:click={() => openCollection(collection)}>
						{#if collection.previewUrl}<img src={collection.previewUrl} alt="" loading="lazy"/>{:else}<span class="image-placeholder" aria-hidden="true">?</span>{/if}
						<span class="text">
							<span class="title">{collection.fields.title}</span>
							<small>{$_('workshop_visibility_' + collection.fields.visibility)} · {$_('collections_item_count', { values: { count: collection.fields.items.length } })} · {$_('collections_subscribers', { values: { count: collection.subscriptions.toLocaleString(numberLocale) } })}{#if collection.favorites != null} · {$_('collections_favorites', { values: { count: collection.favorites.toLocaleString(numberLocale) } })}{/if}</small>
						</span>
						<span class="updated">{$_('collections_updated')} <Timestamp unix={collection.timeUpdated}/></span>
					</button>
				</li>
			{/each}
		</ul>
	</section>
{/if}

{#if deleting}
	<Modal active={true} cancel={() => { if (!deleting.busy) deleting = null; }} padding="1.5rem">
		<div class="dialog" role="dialog" aria-modal="true" aria-labelledby="delete-collections-title">
			<h2 id="delete-collections-title">{$_('collections_delete_title', { values: { count: deleting.ids.length } })}</h2>
			<ul class="names">
				{#each deleting.ids as id (id)}<li>{byId.get(id)?.fields.title ?? id}</li>{/each}
			</ul>
			<p class="warning">{$_('collections_delete_warning')}</p>
			<div class="actions">
				<button type="button" disabled={deleting.busy} on:click={() => deleting = null}>{$_('cancel')}</button>
				<button type="button" class="danger" disabled={deleting.busy} on:click={deleteCollections}>{#if deleting.busy}<Loading inline size="1rem"/>{:else}<Trash2 size="1rem"/>{/if}{$_('collections_delete', { values: { count: deleting.ids.length } })}</button>
			</div>
		</div>
	</Modal>
{/if}

{#if removing}
	<Modal active={true} cancel={cancelRemove} padding="1.5rem">
		<div class="dialog" role="dialog" aria-modal="true" aria-labelledby="remove-everywhere-title">
			<h2 id="remove-everywhere-title">{$_('collections_remove_everywhere_title')}</h2>
			<p>{$_('collections_remove_everywhere_help')}</p>
			<form class="find" on:submit|preventDefault={findRemoval}>
				<input type="search" value={removing.reference} on:input={event => removing = { ...removing, reference: event.currentTarget.value, entry: null }} disabled={removing.busy} placeholder={$_('collections_remove_placeholder')} aria-label={$_('collections_remove_placeholder')}/>
				<button type="submit" disabled={removing.busy || removing.looking || !removing.reference.trim()}>{#if removing.looking}<Loading inline size="1rem"/>{:else}<Search size="1rem"/>{/if}{$_('workshop_search')}</button>
			</form>
			{#if removing.entry}
				<p><strong>{removing.entry.title ?? removing.entry.id}</strong></p>
				{#if !removalChoices.length}
					<p class="muted">{$_('collections_remove_none')}</p>
				{:else}
					<ul class="choices">
						{#each removalChoices as collection (collection.id)}
							<li><label><input type="checkbox" checked={removing.targets.has(collection.id)} disabled={removing.busy} on:change={event => toggleTarget(collection.id, event.currentTarget.checked)}/>{collection.fields.title}</label></li>
						{/each}
					</ul>
				{/if}
			{/if}
			{#if removing.progress}<progress max={removing.progress.total || 1} value={removing.progress.completed} aria-label={$_('collections_saving')}></progress>{/if}
			{#if removing.error}<p class="error-text" role="alert">{removing.error}</p>{/if}
			{#each removing.failures as failure}<p class="error-text" role="alert"><strong>{failure.title}</strong>: {failure.error}</p>{/each}
			<div class="actions">
				<button type="button" disabled={removing.busy} on:click={cancelRemove}>{$_(removing.failures.length ? 'close' : 'cancel')}</button>
				<button type="button" class="danger" disabled={removing.busy || !removing.entry || !removing.targets.size} on:click={removeEverywhere}>{#if removing.busy}<Loading inline size="1rem"/>{:else}<ListMinus size="1rem"/>{/if}{$_('collections_remove_from', { values: { count: removing.targets.size } })}</button>
			</div>
		</div>
	</Modal>
{/if}

<style>
	.collections { display: flex; flex-direction: column; height: 100%; min-height: 0; }
	.toolbar, .panel, .selection-summary, .selection-actions { display: flex; align-items: center; flex-wrap: wrap; gap: .75rem; }
	.toolbar { padding: 1rem 1.5rem .5rem; }
	.panel { gap: .5rem 1rem; margin: .5rem 1.5rem 0; padding: .5rem; padding-inline-start: 1rem; border: 1px solid var(--border); border-radius: 4px; background: var(--bg-panel); }
	.selection-summary, .selection-actions { gap: .5rem; }
	.selection-actions { margin-inline-start: auto; }
	.panel button, .panel select { padding: .45rem .7rem; }
	.spacer { flex: 1; }
	button, select, input { border: 1px solid var(--border-strong); border-radius: 4px; padding: .6rem .75rem; background: var(--control); color: var(--text); font: inherit; }
	button { display: inline-flex; align-items: center; gap: .5rem; cursor: pointer; }
	select { cursor: pointer; }
	button:hover:not(:disabled), select:hover:not(:disabled) { background: var(--control-hover); }
	button:disabled, select:disabled { opacity: .45; cursor: default; }
	button.active { border-color: var(--neutral); background: var(--neutral-dark); color: #fff; }
	button.active:hover:not(:disabled) { background: var(--neutral); }
	button.primary { border-color: var(--neutral); background: var(--neutral); color: #fff; }
	button.primary:hover:not(:disabled) { background: var(--neutral-dark); }
	button.danger { border-color: var(--error); background: var(--error); color: #fff; }
	button.danger:hover:not(:disabled) { background: var(--error-dark); }
	button.plain { border-color: transparent; background: none; color: var(--link); }
	button.plain:hover:not(:disabled) { background: var(--control); }
	.icon-button { padding: .6rem; }
	button:focus-visible, select:focus-visible, input:focus-visible { outline: 2px solid var(--text); outline-offset: 2px; }
	input[type='checkbox'] { accent-color: var(--neutral); }
	option { background: var(--control); color: var(--text); }
	.filters { position: relative; }
	fieldset { position: absolute; top: calc(100% + .5rem); left: 0; z-index: 5; min-width: 200px; margin: 0; padding: .75rem; border: 1px solid var(--border-strong); border-radius: 4px; background: var(--bg-raised); box-shadow: 0 6px 18px #0008; }
	legend { padding: 0 .35rem; background: var(--bg-raised); }
	fieldset label { display: flex; align-items: center; gap: .5rem; padding: .5rem .25rem; cursor: pointer; }
	.sort { display: flex; align-items: center; flex-wrap: wrap; gap: .5rem; }
	.search { display: flex; align-items: center; gap: .5rem; padding-inline-start: .6rem; border: 1px solid var(--border-strong); border-radius: 4px; background: var(--control); color: var(--text-muted); }
	.search input { border: 0; background: none; min-width: 10rem; padding-inline-start: 0; }
	.search input:focus-visible { outline: none; }
	.search:focus-within { outline: 2px solid var(--text); outline-offset: 2px; }
	.count { font-weight: 600; }
	.status, .error, .notice { margin: .5rem 1.5rem 0; }
	.status { display: flex; align-items: center; gap: .5rem; color: var(--text-soft); }
	.notice { color: var(--text-soft); }
	.error { color: var(--text-error-soft); overflow-wrap: anywhere; }
	.error p { margin: 0 0 .5rem; }
	.failures { padding-inline-start: 1.25rem; line-height: 1.5; }
	.list { flex: 1; min-height: 0; overflow-y: auto; list-style: none; margin: .75rem 0 0; padding: 0 1.5rem 1.5rem; }
	.list li { display: flex; align-items: center; gap: .75rem; margin-bottom: .5rem; padding-inline-start: .25rem; border-radius: 4px; }
	.list li.selected { background: color-mix(in srgb, var(--neutral) 18%, transparent); box-shadow: inset 0 0 0 2px var(--neutral); }
	.list li input { margin-inline-start: .5rem; }
	.row { flex: 1; min-width: 0; display: flex; align-items: center; gap: 1rem; padding: .6rem .75rem; border: 0; background: var(--bg-preview); text-align: start; }
	.list li.selected .row { background: transparent; }
	.row:hover:not(:disabled) { background: var(--bg-hover); }
	.row:focus-visible { outline: 2px solid #127cff; outline-offset: 1px; }
	.row img, .image-placeholder { width: 3rem; height: 3rem; border-radius: 4px; object-fit: cover; flex-shrink: 0; background: var(--control); }
	.image-placeholder { display: grid; place-items: center; color: var(--text-muted); }
	.text { flex: 1; min-width: 0; display: flex; flex-direction: column; }
	.title { font-weight: 600; overflow-wrap: anywhere; }
	small { color: var(--text-muted); font-size: .8rem; margin-top: .25rem; }
	.updated { color: var(--text-muted); font-size: .8rem; white-space: nowrap; }
	.dialog { width: min(30rem, 70vw); }
	.dialog h2 { margin: 0 0 .75rem; font-size: 1.1rem; overflow-wrap: anywhere; }
	.dialog p { margin: 0 0 1rem; line-height: 1.5; overflow-wrap: anywhere; }
	.muted { color: var(--text-muted); }
	.names, .choices { max-height: 12rem; overflow-y: auto; margin: 0 0 1rem; padding: .5rem .75rem; border: 1px solid var(--border); border-radius: 4px; font-size: .85rem; line-height: 1.5; overflow-wrap: anywhere; }
	.names { padding-inline-start: 1.75rem; }
	.choices { list-style: none; }
	.choices label { display: flex; align-items: center; gap: .5rem; padding: .25rem 0; cursor: pointer; }
	.warning { border-inline-start: 3px solid var(--error); padding-inline-start: .75rem; }
	.find { display: flex; gap: .5rem; margin-bottom: 1rem; }
	.find input { flex: 1; min-width: 0; }
	.dialog progress { width: 100%; height: .5rem; accent-color: var(--neutral); margin-bottom: 1rem; }
	.error-text { color: var(--text-error-soft); }
	.actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: .75rem; margin-top: 1rem; }
</style>
