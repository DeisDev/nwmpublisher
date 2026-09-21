<script>
	import { Steam } from '../steam';
	import AddonScroller from '../components/AddonScroller.svelte';
	import { _ } from 'svelte-i18n';
	import { writable } from 'svelte/store';
	import PreparePublish, { remountAddonScroller } from '../components/PreparePublish.svelte';
	import { afterUpdate, onDestroy } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import Filter from '@lucide/svelte/icons/list-filter';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Loading from '../components/Loading.svelte';

	const visibilityOptions = ['public', 'friends_only', 'private', 'unlisted'];
	const sortOptions = ['updated_desc', 'updated_asc', 'subscribers_desc', 'subscribers_asc', 'created_desc', 'created_asc', 'title_asc', 'title_desc'];
	let visibility = [...AppSettings.my_workshop_visibility];
	let sort = AppSettings.my_workshop_sort;
	let saving = false;
	let saveError = null;
	let filtersOpen = false;
	let filterMenu;
	let filterButton;
	let addons = [];
	let loading = true;
	let loadedCount = 0;
	let total = 0;
	let loadError = null;
	let loadId = 0;

	$: visibleAddons = selectAddons(addons, visibility, sort);

	function selectAddons(items, selectedVisibility, selectedSort) {
		const [field, direction] = selectedSort.split('_');
		const multiplier = direction === 'asc' ? 1 : -1;
		return items.filter(addon => selectedVisibility.includes(addon.workshop.visibility)).sort((a, b) => {
			const left = a.workshop;
			const right = b.workshop;
			let order;
			if (field === 'title') {
				order = left.title.localeCompare(right.title, undefined, { numeric: true, sensitivity: 'base' });
			} else {
				const property = { updated: 'timeUpdated', created: 'timeCreated', subscribers: 'subscriptions' }[field];
				order = left[property] - right[property];
			}
			return order * multiplier || left.id - right.id;
		});
	}

	async function loadAddons() {
		const requestId = ++loadId;
		loading = true;
		loadError = null;
		addons = [];
		loadedCount = 0;
		total = 0;
		const fetched = [];
		try {
			// Sort only after every Steam page is loaded, so subscriber ordering is global.
			for (let page = 1; ; page++) {
				const [fetchedTotal, items] = await Steam.getMyWorkshop(page);
				if (requestId !== loadId) return;
				total = fetchedTotal;
				fetched.push(...items);
				loadedCount = fetched.length;
				if (loadedCount >= total) break;
				if (items.length === 0) throw new Error($_('workshop_incomplete'));
			}
			addons = fetched;
		} catch (error) {
			if (requestId === loadId) loadError = String(error);
		} finally {
			if (requestId === loadId) loading = false;
		}
	}
	loadAddons();
	onDestroy(() => loadId++);

	function retryLoad() {
		Steam.MyWorkshop = [];
		loadAddons();
	}

	async function savePreferences(nextVisibility, nextSort) {
		if (saving) return;
		const previousVisibility = visibility;
		const previousSort = sort;
		visibility = nextVisibility;
		sort = nextSort;
		saving = true;
		saveError = null;
		try {
			await invoke('update_settings', { settings: {
				...AppSettings,
				my_workshop_visibility: nextVisibility,
				my_workshop_sort: nextSort,
			} });
			AppSettings.my_workshop_visibility = nextVisibility;
			AppSettings.my_workshop_sort = nextSort;
		} catch (error) {
			visibility = previousVisibility;
			sort = previousSort;
			saveError = String(error);
		} finally {
			saving = false;
		}
	}

	function toggleVisibility(value, checked) {
		savePreferences(checked ? [...visibility, value] : visibility.filter(item => item !== value), sort);
	}

	function dismissFilters(event) {
		if (filtersOpen && !filterMenu.contains(event.target)) filtersOpen = false;
	}

	function onKeydown(event) {
		if (filtersOpen && event.key === 'Escape') {
			filtersOpen = false;
			filterButton.focus();
		}
	}

	let updatingAddon = writable(null);
	let preparePublish = writable(false);

	function togglePreparePublish() {
		$updatingAddon = null;
		$preparePublish = !$preparePublish;
	}

	async function editPublishedAddon(_, addon) {
		const addonAwaited = await addon;
		if (addonAwaited != $updatingAddon) {
			$updatingAddon = addonAwaited;
		}
		$preparePublish = !$preparePublish;
	}

	afterUpdate(() => {
		if ($remountAddonScroller) {
			$remountAddonScroller = false;
			loadAddons();
		}
	});
</script>

<svelte:window on:pointerdown={dismissFilters} on:keydown={onKeydown}/>

<section class="workshop">
	<div class="toolbar">
		<div class="filters" bind:this={filterMenu}>
			<button type="button" class:active={visibility.length !== visibilityOptions.length} aria-expanded={filtersOpen} aria-controls="workshop-filters" bind:this={filterButton} on:click={() => filtersOpen = !filtersOpen}>
				<Filter size="1rem"/>
				{$_('workshop_filters')} ({visibility.length}/{visibilityOptions.length})
				<ChevronDown size="1rem"/>
			</button>
			{#if filtersOpen}
				<fieldset id="workshop-filters" disabled={saving}>
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
			<select value={sort} disabled={saving} on:change={event => savePreferences(visibility, event.currentTarget.value)}>
				{#each sortOptions as value}
					<option {value}>{$_('workshop_sort_' + value)}</option>
				{/each}
			</select>
		</label>
	</div>
	{#if saveError}
		<p class="error" role="alert">{$_('workshop_save_error', { values: { error: saveError } })}</p>
	{/if}
	{#if loading}
		<p class="status" role="status"><Loading inline size="1rem"/> {$_('workshop_loading', { values: { num: loadedCount, total } })}</p>
	{:else if loadError}
		<div class="error" role="alert">
			<p>{$_('workshop_load_error', { values: { error: loadError } })}</p>
			<button type="button" on:click={retryLoad}>{$_('workshop_retry')}</button>
		</div>
	{:else}
		<p class="status" role="status">
			{#if visibility.length === 0}
				{$_('workshop_no_visibility')}
			{:else if visibleAddons.length === 0}
				{$_('workshop_no_matches')}
			{:else}
				{$_('showing_num_addons', { values: { num: visibleAddons.length.toLocaleString(), total: addons.length.toLocaleString() } })}
			{/if}
		</p>
	{/if}
	<div class="results">
		{#key visibleAddons}
			<AddonScroller next={() => Promise.resolve([visibleAddons.length, visibleAddons])} onClick={editPublishedAddon} onNewAddonClick={togglePreparePublish}/>
		{/key}
	</div>
</section>

<PreparePublish {preparePublish} {updatingAddon}/>

<style>
	.workshop {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}
	.toolbar {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: .75rem;
		padding: 1rem 1.5rem .5rem;
	}
	button, select {
		border: 1px solid #484848;
		border-radius: 4px;
		padding: .6rem .75rem;
		background: #313131;
		color: #fff;
		font: inherit;
		cursor: pointer;
	}
	button {
		display: inline-flex;
		align-items: center;
		gap: .5rem;
	}
	button:hover, select:hover {
		background: #414141;
	}
	button.active {
		border-color: var(--neutral);
		background: var(--neutral-dark);
	}
	button:focus-visible, select:focus-visible, input:focus-visible {
		outline: 2px solid #fff;
		outline-offset: 2px;
	}
	.filters {
		position: relative;
	}
	fieldset {
		position: absolute;
		top: calc(100% + .5rem);
		left: 0;
		z-index: 5;
		min-width: 200px;
		margin: 0;
		padding: .75rem;
		border: 1px solid #484848;
		border-radius: 4px;
		background: #252525;
		box-shadow: 0 6px 18px #0008;
	}
	legend {
		padding: 0 .35rem;
		background: #252525;
	}
	fieldset label {
		display: flex;
		align-items: center;
		gap: .5rem;
		padding: .5rem .25rem;
		cursor: pointer;
	}
	input {
		accent-color: var(--neutral);
	}
	.sort {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: .5rem;
	}
	select:disabled, fieldset:disabled {
		opacity: .6;
	}
	.status, .error {
		margin: .5rem 1.5rem 0;
	}
	.status {
		color: #bbb;
	}
	.error {
		color: #ffb6b6;
	}
	.results {
		flex: 1;
		min-height: 0;
	}
</style>
