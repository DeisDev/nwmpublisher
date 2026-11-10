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
	import ListX from '@lucide/svelte/icons/list-x';
	import CircleStop from '@lucide/svelte/icons/circle-stop';
	import Loading from '../components/Loading.svelte';
	import Modal from '../components/Modal.svelte';
	import Timestamp from '../components/Timestamp.svelte';
	import CollectionEditor from '../components/CollectionEditor.svelte';
	import CollectionCompare from '../components/CollectionCompare.svelte';
	import { collections, compareTitles, ensureCollections, includeExtras, loadCollections, loadSnapshot, mirrors, planSync, setIncludeExtras, subscriptionSnapshot } from '../collections.js';
	import { saveSettings } from '../settings.js';
	import { playSound } from '../sounds';
	import { taskError, taskMessage } from '../transactions.js';

	const visibilityOptions = ['public', 'friends_only', 'private', 'unlisted'];
	const sortOptions = ['updated_desc', 'updated_asc', 'created_desc', 'created_asc', 'title_asc', 'title_desc', 'items_desc', 'items_asc', 'visitors_desc', 'visitors_asc'];
	let visibility = [...AppSettings.collections_visibility];
	let sort = AppSettings.collections_sort;
	let saved = { visibility, sort };
	let saveRequest = 0;
	let filtersOpen = false;
	let filterMenu;
	let filterButton;
	let query = '';
	let view = null;
	let selecting = false;
	let selected = new Set();
	let actionBusy = false;

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
			else if (field === 'visitors') order = (a.views ?? 0) - (b.views ?? 0);
			else {
				const property = { updated: 'timeUpdated', created: 'timeCreated' }[field];
				order = a[property] - b[property];
			}
			return order * multiplier || a.id.length - b.id.length || a.id.localeCompare(b.id);
		});
	}

	function savePreferences(nextVisibility, nextSort) {
		const request = ++saveRequest;
		visibility = nextVisibility;
		sort = nextSort;
		saveSettings({
			collections_visibility: nextVisibility,
			collections_sort: nextSort,
		}).then(() => {
			saved = { visibility: nextVisibility, sort: nextSort };
			AppSettings.collections_visibility = nextVisibility;
			AppSettings.collections_sort = nextSort;
		}, () => {
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
		view = { kind: 'edit', id: collection.id };
	}

	function newCollection() {
		view = { kind: 'new' };
	}

	async function created({ detail }) {
		await loadCollections();
		view = get(collections).items.some(collection => collection.id === detail.id) ? { kind: 'edit', id: detail.id } : null;
		if (detail.error) taskError(detail.error);
		else if (!view) taskMessage($_('collections_created_pending'));
	}

	function failureList(outcomes) {
		return outcomes.filter(outcome => outcome.error).map(outcome => ({ title: byId.get(outcome.id)?.fields.title ?? outcome.id, error: outcome.error }));
	}

	function reportFailures(failures) {
		if (failures.length) taskError(failures.map(failure => `${failure.title}: ${failure.error}`).join('\n'));
	}

	async function changeVisibility(value) {
		const ids = selectedList;
		if (!value || !ids.length || actionBusy) return;
		actionBusy = true;
		try {
			const outcomes = await invoke('set_collections_visibility', { collections: ids, visibility: value });
			const failures = failureList(outcomes);
			reportFailures(failures);
			const changed = outcomes.length - failures.length;
			if (changed) {
				taskMessage($_('collections_visibility_changed', { values: { count: changed } }));
				playSound('success');
			}
		} catch (error) {
			reportFailures([{ title: $_('collections'), error: String(error) }]);
		} finally {
			actionBusy = false;
			loadCollections();
		}
	}

	let deleting = null;

	function confirmDelete(ids) {
		deleting = { ids, busy: false };
	}

	async function deleteCollections() {
		const ids = deleting.ids;
		deleting = { ...deleting, busy: true };
		try {
			const outcomes = await invoke('delete_collections', { collections: ids });
			reportFailures(failureList(outcomes));
			const removed = outcomes.filter(outcome => !outcome.error).map(outcome => outcome.id);
			if (removed.length) {
				taskMessage($_('collections_deleted', { values: { count: removed.length } }));
				playSound('success');
			}
			selected = new Set([...selected].filter(id => !removed.includes(id)));
			if (view?.kind === 'edit' && removed.includes(view.id)) view = null;
		} catch (error) {
			reportFailures([{ title: $_('collections'), error: String(error) }]);
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
			const { outcomes } = await invoke('apply_collection_changes', {
				changes: [...targets].map(collectionId => ({ collectionId, remove: [entry.id] })),
				onProgress: new Channel(progress => { if (removing) removing = { ...removing, progress }; }),
			});
			const failures = failureList(outcomes);
			const changed = outcomes.filter(outcome => outcome.changed.length).length;
			if (changed) {
				taskMessage($_('collections_removed_everywhere', { values: { count: changed } }));
				playSound('success');
			}
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

	let pruning = null;

	async function openPrune() {
		const ids = selectedList;
		pruning = { ids, loading: true, error: null, entries: new Map(), unticked: new Set(), busy: false, stopping: false, progress: null, failures: [], notice: null };
		await loadSnapshot();
		const state = get(subscriptionSnapshot);
		if (!pruning) return;
		if (state.error) {
			pruning = { ...pruning, loading: false, error: state.error };
			return;
		}
		const subscribed = new Set(state.data.items.map(entry => entry.id));
		const missing = [...new Set(ids.flatMap(id => byId.get(id)?.fields.items ?? []))].filter(item => !subscribed.has(item));
		try {
			const lookup = missing.length ? await invoke('collection_entries', { references: missing }) : { entries: [] };
			if (pruning) pruning = { ...pruning, loading: false, entries: new Map(lookup.entries.map(entry => [entry.id, entry])) };
		} catch (error) {
			if (pruning) pruning = { ...pruning, loading: false, error: String(error) };
		}
	}

	function togglePrune(id, checked) {
		const unticked = new Set(pruning.unticked);
		if (checked) unticked.delete(id);
		else unticked.add(id);
		pruning = { ...pruning, unticked };
	}

	async function prune() {
		const chosen = pruneChoices.filter(choice => choice.items.length && !pruning.unticked.has(choice.id));
		pruning = { ...pruning, busy: true, stopping: false, error: null, failures: [], notice: null, progress: { completed: 0, total: pruneCount } };
		try {
			const local = new Set(await invoke('local_subscribed_ids'));
			const changes = chosen.map(choice => ({ collectionId: choice.id, remove: choice.items.filter(item => !local.has(item)) })).filter(change => change.remove.length);
			const resubscribed = chosen.reduce((count, choice) => count + choice.items.length, 0) - changes.reduce((count, change) => count + change.remove.length, 0);
			const notice = resubscribed ? $_('collections_sync_resubscribed', { values: { count: resubscribed } }) : null;
			if (!changes.length) {
				pruning = { ...pruning, busy: false, progress: null, notice };
				return;
			}
			const result = await invoke('apply_collection_changes', {
				changes,
				onProgress: new Channel(progress => { if (pruning) pruning = { ...pruning, progress }; }),
			});
			const failures = failureList(result.outcomes);
			const changed = result.outcomes.filter(outcome => outcome.changed.length);
			const removed = changed.reduce((count, outcome) => count + outcome.changed.length, 0);
			if (removed) {
				taskMessage($_('collections_sync_pruned', { values: { items: removed, collections: changed.length } }));
				playSound('success');
			}
			const stopped = result.stopped ? $_('collections_sync_prune_stopped', { values: { count: removed } }) : null;
			const messages = [notice, stopped].filter(Boolean).join(' ') || null;
			pruning = failures.length || messages ? { ...pruning, busy: false, progress: null, failures, notice: messages } : null;
		} catch (error) {
			pruning = { ...pruning, busy: false, progress: null, error: String(error) };
		} finally {
			loadCollections();
		}
	}

	function stopPrune() {
		pruning = { ...pruning, stopping: true };
		invoke('stop_collection_change');
	}

	function cancelPrune() {
		if (!pruning?.busy) pruning = null;
	}

	$: pruneChoices = pruning && !pruning.loading && !pruning.error && $subscriptionSnapshot.data
		? pruning.ids.filter(id => byId.has(id)).map(id => ({ id, title: byId.get(id).fields.title, items: planSync(id, byId.get(id).fields.items, pruning.entries, $subscriptionSnapshot.data.items, $includeExtras).remove }))
		: [];
	$: pruneTargets = pruneChoices.filter(choice => choice.items.length && !pruning.unticked.has(choice.id));
	$: pruneCount = pruneTargets.reduce((count, choice) => count + choice.items.length, 0);
</script>

<svelte:window on:pointerdown={dismissFilters} on:keydown={onKeydown}/>

{#if view?.kind === 'edit' && byId.has(view.id)}
	{#key view.id}
		<CollectionEditor collection={byId.get(view.id)} others={all.filter(collection => collection.id !== view.id)} on:close={() => view = null} on:saved={loadCollections} on:delete={event => confirmDelete([event.detail])} on:removeEverywhere={event => openRemove(event.detail)}/>
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
					<button type="button" disabled={!selectedList.length || actionBusy} on:click={openPrune}><ListX size="1rem"/>{$_('collections_sync_prune')}</button>
					<button type="button" disabled={!selectedList.length || actionBusy} on:click={() => confirmDelete(selectedList)}><Trash2 size="1rem"/>{$_('collections_delete', { values: { count: selectedList.length } })}</button>
					{#if actionBusy}<Loading inline size="1rem"/>{/if}
				</div>
			</div>
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
							<small>{$_('workshop_visibility_' + collection.fields.visibility)} · {$_('collections_item_count', { values: { count: collection.fields.items.length } })}{#if collection.views != null}{' · '}{$_('collections_visitors', { values: { count: collection.views.toLocaleString(numberLocale) } })}{/if}{#if collection.favorites != null}{' · '}{$_('collections_favorites', { values: { count: collection.favorites.toLocaleString(numberLocale) } })}{/if}{#if $mirrors.has(collection.id)}{' · '}<span class="chip">{$_('collections_sync_mirror_chip')}</span>{/if}</small>
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

{#if pruning}
	<Modal active={true} cancel={cancelPrune} padding="1.5rem">
		<div class="dialog" role="dialog" aria-modal="true" aria-labelledby="prune-title">
			<h2 id="prune-title">{$_('collections_sync_prune_title')}</h2>
			<p>{$_('collections_sync_prune_help')}</p>
			<label class="option"><input type="checkbox" checked={$includeExtras} disabled={pruning.busy} on:change={event => setIncludeExtras(event.currentTarget.checked).catch(error => pruning = pruning && { ...pruning, error: String(error) })}/>{$_('collections_sync_include_extras')}</label>
			{#if pruning.loading}
				{@const progress = $subscriptionSnapshot.progress}
				<p class="status-line" role="status"><Loading inline size="1rem"/> {progress?.total ? $_('collections_sync_loading_count', { values: { completed: progress.completed, total: progress.total } }) : $_('collections_sync_loading')}</p>
			{:else if pruneChoices.length}
				<ul class="choices">
					{#each pruneChoices as choice (choice.id)}
						<li class:muted={!choice.items.length}><label><input type="checkbox" checked={choice.items.length > 0 && !pruning.unticked.has(choice.id)} disabled={pruning.busy || !choice.items.length} on:change={event => togglePrune(choice.id, event.currentTarget.checked)}/>{$_('collections_sync_prune_choice', { values: { title: choice.title, count: choice.items.length } })}</label></li>
					{/each}
				</ul>
				<p class="warning">{$_('collections_sync_prune_warning')}</p>
			{/if}
			{#if pruning.progress}
				<div class="progress-row">
					<progress max={pruning.progress.total || 1} value={pruning.progress.completed} aria-label={$_('collections_saving')}></progress>
					<button type="button" disabled={pruning.stopping} on:click={stopPrune}><CircleStop size="1rem"/>{$_('collections_stop')}</button>
				</div>
				{#if pruning.progress.retryIn}<p class="muted" role="status">{$_('collections_save_throttled', { values: { seconds: pruning.progress.retryIn } })}</p>{/if}
			{/if}
			{#if pruning.notice}<p role="status">{pruning.notice}</p>{/if}
			{#if pruning.error}<p class="error-text" role="alert">{pruning.error}</p>{/if}
			{#each pruning.failures as failure}<p class="error-text" role="alert"><strong>{failure.title}</strong>: {failure.error}</p>{/each}
			<div class="actions">
				<button type="button" disabled={pruning.busy} on:click={cancelPrune}>{$_(pruning.failures.length || pruning.notice ? 'close' : 'cancel')}</button>
				<button type="button" class="danger" disabled={pruning.busy || pruning.loading || !pruneCount} on:click={prune}>{#if pruning.busy}<Loading inline size="1rem"/>{:else}<ListX size="1rem"/>{/if}{$_('collections_sync_prune_button', { values: { items: pruneCount, collections: pruneTargets.length } })}</button>
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
	.status, .error { margin: .5rem 1.5rem 0; }
	.status { display: flex; align-items: center; gap: .5rem; color: var(--text-soft); }
	.error { color: var(--text-error-soft); overflow-wrap: anywhere; }
	.error p { margin: 0 0 .5rem; }
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
	.chip { padding: .05rem .4rem; border-radius: 4px; background: var(--control); }
	.option { display: flex; align-items: center; gap: .5rem; margin-bottom: 1rem; cursor: pointer; }
	.status-line { display: flex; align-items: center; gap: .5rem; color: var(--text-soft); }
	.choices li.muted { color: var(--text-muted); }
	.progress-row { display: flex; align-items: center; gap: .75rem; margin-bottom: 1rem; }
	.progress-row progress { flex: 1; margin: 0; }
	.progress-row button { padding: .4rem .65rem; }
	.actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: .75rem; margin-top: 1rem; }
</style>
