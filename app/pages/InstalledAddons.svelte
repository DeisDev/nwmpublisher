<script>
	import { writable } from 'svelte/store';
	import { _ } from 'svelte-i18n';
	import { Steam, installedAddonsRevision } from '../steam';
	import { saveSettings } from '../settings.js';
	import Filter from '@lucide/svelte/icons/list-filter';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import AddonScroller from '../components/AddonScroller.svelte';
	import PreviewGMA from '../components/PreviewGMA.svelte';

	const typeOptions = ['gamemode', 'map', 'weapon', 'vehicle', 'npc', 'entity', 'tool', 'effects', 'model', 'ServerContent', 'other'];
	const sortOptions = ['modified', 'type'];
	let types = [...AppSettings.installed_addons_types];
	let sort = AppSettings.installed_addons_sort;
	let saved = { types, sort };
	let saveRequest = 0;
	let saveError = null;
	let filtersOpen = false;
	let filterMenu;
	let filterButton;

	function pager(selectedSort, selectedTypes) {
		let page = 0;
		return () => Steam.getInstalledAddons(++page, selectedSort, selectedTypes);
	}
	let next;
	$: $installedAddonsRevision, next = pager(sort, types);

	function savePreferences(nextTypes, nextSort) {
		const request = ++saveRequest;
		types = nextTypes;
		sort = nextSort;
		saveError = null;
		saveSettings({
			installed_addons_types: nextTypes,
			installed_addons_sort: nextSort,
		}).then(() => {
			saved = { types: nextTypes, sort: nextSort };
			AppSettings.installed_addons_types = nextTypes;
			AppSettings.installed_addons_sort = nextSort;
		}, error => {
			saveError = String(error);
			if (request !== saveRequest) return;
			types = saved.types;
			sort = saved.sort;
		});
	}

	function toggleType(value, checked) {
		savePreferences(typeOptions.filter(option => option === value ? checked : types.includes(option)), sort);
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

	let previewingGMA = false;
	const promises = writable([new Promise(() => {}), new Promise(() => {})]);
	function onClick(_, workshop, installed) {
		previewingGMA = true;
		$promises = [workshop, installed];
	}
</script>

<svelte:window on:pointerdown={dismissFilters} on:keydown={onKeydown}/>

<section class="installed">
	<div class="toolbar">
		<div class="filters" bind:this={filterMenu}>
			<button type="button" class:active={types.length !== typeOptions.length} aria-expanded={filtersOpen} aria-controls="installed-filters" bind:this={filterButton} on:click={() => filtersOpen = !filtersOpen}>
				<Filter size="1rem"/>
				{$_('workshop_filters')} ({types.length}/{typeOptions.length})
				<ChevronDown size="1rem"/>
			</button>
			{#if filtersOpen}
				<fieldset id="installed-filters">
					<legend>{$_('installed_types')}</legend>
					{#each typeOptions as value}
						<label>
							<input type="checkbox" checked={types.includes(value)} on:change={event => toggleType(value, event.currentTarget.checked)}/>
							{$_('addon_types.' + value)}
						</label>
					{/each}
				</fieldset>
			{/if}
		</div>
		<label class="sort">
			<span>{$_('workshop_sort')}</span>
			<select value={sort} on:change={event => savePreferences(types, event.currentTarget.value)}>
				{#each sortOptions as value}
					<option {value}>{$_('installed_sort_' + value)}</option>
				{/each}
			</select>
		</label>
	</div>
	{#if saveError}
		<p class="error" role="alert">{$_('installed_save_error', { values: { error: saveError } })}</p>
	{/if}
	{#if types.length === 0}
		<p class="status" role="status">{$_('installed_no_types')}</p>
	{:else}
		<div class="results">
			<AddonScroller {next} {onClick}/>
		</div>
	{/if}
</section>

<PreviewGMA active={previewingGMA} {promises} cancel={() => previewingGMA = false}/>

<style>
	.installed {
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
		border: 1px solid var(--border-strong);
		border-radius: 4px;
		padding: .6rem .75rem;
		background: var(--control);
		color: var(--text);
		font: inherit;
		cursor: pointer;
	}
	button {
		display: inline-flex;
		align-items: center;
		gap: .5rem;
	}
	button:hover, select:hover {
		background: var(--control-hover);
	}
	button.active {
		border-color: var(--neutral);
		background: var(--neutral-dark);
		color: #fff;
	}
	button:focus-visible, select:focus-visible, input:focus-visible {
		outline: 2px solid var(--text);
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
		border: 1px solid var(--border-strong);
		border-radius: 4px;
		background: var(--bg-raised);
		box-shadow: 0 6px 18px #0008;
	}
	legend {
		padding: 0 .35rem;
		background: var(--bg-raised);
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
	.status, .error {
		margin: .5rem 1.5rem 0;
	}
	.status {
		color: var(--text-soft);
	}
	.error {
		color: var(--text-error-soft);
	}
	.results {
		flex: 1;
		min-height: 0;
	}
</style>
