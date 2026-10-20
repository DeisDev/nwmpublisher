<script>
	import { tick } from 'svelte';
	import { writable } from 'svelte/store';
	import { _ } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import { message } from '@tauri-apps/plugin-dialog';
	import { Steam, installedAddonsRevision } from '../steam';
	import { saveSettings, settings } from '../settings.js';
	import { formatSize } from '../format.js';
	import { tippy } from '../tippy.js';
	import { taskMessage } from '../transactions.js';
	import { addonName, addonRemoval, addonRemovals, confirmRemoval, deleteRequest, removalJob, removedPaths, stopRemoval } from '../addon-removal.js';
	import { createSelection } from '../addon-selection.js';
	import Filter from '@lucide/svelte/icons/list-filter';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import SquareCheck from '@lucide/svelte/icons/square-check';
	import FolderAdd from '@lucide/svelte/icons/folder-plus';
	import CircleMinus from '@lucide/svelte/icons/circle-minus';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import LinkChain from '@lucide/svelte/icons/link';
	import Folder from '@lucide/svelte/icons/folder';
	import AddonScroller from '../components/AddonScroller.svelte';
	import PreviewGMA from '../components/PreviewGMA.svelte';
	import DestinationSelect from '../components/DestinationSelect.svelte';

	const typeOptions = ['gamemode', 'map', 'weapon', 'vehicle', 'npc', 'entity', 'tool', 'effects', 'model', 'ServerContent', 'other'];
	const sortOptions = ['modified', 'type'];
	const MAX_LOCATIONS = 10;
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
		selectAllRequest++;
		selection.clear();
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

	let section;
	function onKeydown(event) {
		if (event.defaultPrevented) return;
		if (filtersOpen && event.key === 'Escape') {
			filtersOpen = false;
			filterButton.focus();
			return;
		}
		if (event.altKey || !section?.contains(event.target) || event.target.closest('input, select, textarea')) return;
		if (event.key === 'Escape' && $selection.active) {
			event.preventDefault();
			selection.stop();
		} else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
			event.preventDefault();
			selectAll();
		}
	}

	let scroller;
	let total = 0;
	let returnFocus = null;

	function rememberFocus() {
		returnFocus = document.activeElement;
	}

	async function restoreFocus() {
		await tick();
		if (document.activeElement && document.activeElement !== document.body) return;
		if (returnFocus?.isConnected && returnFocus.getAttribute('role') !== 'option') returnFocus.focus({ preventScroll: true });
		else scroller?.focusCurrent();
		returnFocus = null;
	}

	let previewingGMA = false;
	const promises = writable([new Promise(() => {}), new Promise(() => {})]);
	function onClick(_, workshop, installed) {
		rememberFocus();
		previewingGMA = true;
		$promises = [workshop, installed];
	}

	function closePreview() {
		previewingGMA = false;
		restoreFocus();
	}

	const selection = createSelection();
	$: selectedPaths = [...$selection.items.keys()];
	$: selectedCount = selectedPaths.length;
	$: selectedSize = [...$selection.items.values()].reduce((size, gma) => size + (gma.size ?? 0), 0);
	$: selection.remove($removedPaths);

	let selectAllRequest = 0;
	let selectionError = null;
	async function selectAll() {
		const request = ++selectAllRequest;
		selectionError = null;
		try {
			const addons = await invoke('browse_all_installed_addons', { sort, types });
			if (request === selectAllRequest) selection.setAll(addons.map(addon => [addon.installed.path, addon.installed]));
		} catch (error) {
			if (request === selectAllRequest) selectionError = $_('select_all_failed', { values: { error: String(error) } });
		}
	}

	function toggleSelecting() {
		if ($selection.active) selection.stop();
		else selection.start();
	}

	let removals = new Map();
	let removalsRevision = null;
	let removalsRequest = 0;
	let removalsTimer;
	function loadRemovals(paths, revision) {
		if (revision !== removalsRevision) {
			removals = new Map();
			removalsRevision = revision;
		}
		clearTimeout(removalsTimer);
		const missing = paths.filter(path => !removals.has(path));
		if (!missing.length) return;
		const request = ++removalsRequest;
		removalsTimer = setTimeout(() => {
			addonRemovals(missing).then(values => {
				if (request !== removalsRequest || revision !== removalsRevision) return;
				const known = new Map(removals);
				missing.forEach((path, index) => known.set(path, values[index]));
				removals = known;
			}, error => console.error(error));
		}, 100);
	}
	$: loadRemovals(selectedPaths, $installedAddonsRevision);
	$: unsubscribePaths = selectedPaths.filter(path => removals.get(path) === 'unsubscribe');
	$: deletePaths = selectedPaths.filter(path => removals.get(path) === 'delete');
	$: blockedCount = selectedPaths.filter(path => removals.has(path) && !removals.get(path)).length;
	$: linkIds = [...new Set([...$selection.items.values()].map(gma => gma.id).filter(id => id != null))];
	$: linklessCount = [...$selection.items.values()].filter(gma => gma.id == null).length;
	$: folders = new Map(selectedPaths.map(path => [path.slice(0, Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))), path]));
	$: tooManyFolders = folders.size > MAX_LOCATIONS;
	$: unsubscribeLabel = $_($settings.unsubscribe_delete_files ? 'bulk_unsubscribe_delete' : 'bulk_unsubscribe', { values: { count: unsubscribePaths.length } });
	$: deleteLabel = $_('bulk_delete', { values: { count: deletePaths.length } });
	$: extractLabel = $_('bulk_extract_button', { values: { count: selectedCount } });
	$: bulkJob = $removalJob?.total > 1 ? $removalJob : null;

	let awaitingDialog = false;
	$: if (awaitingDialog && !$deleteRequest) {
		awaitingDialog = false;
		restoreFocus();
	}

	function openRemoval(items, unsubscribe) {
		rememberFocus();
		awaitingDialog = true;
		confirmRemoval(items, unsubscribe);
	}

	function removeSelected(unsubscribe) {
		const paths = unsubscribe ? unsubscribePaths : deletePaths;
		if (!paths.length || $removalJob) return;
		openRemoval(paths.map(path => ({ path, name: addonName(null, $selection.items.get(path)) })), unsubscribe);
	}

	let unsubscribeButton;
	async function deleteKey(addon) {
		if ($removalJob) return;
		if (selectedCount) {
			if (unsubscribePaths.length && deletePaths.length) unsubscribeButton?.focus();
			else if (unsubscribePaths.length || deletePaths.length) removeSelected(unsubscribePaths.length > 0);
			return;
		}
		const path = addon.installed.path;
		const removal = await addonRemoval(path);
		if (removal) openRemoval([{ path, name: addonName(addon.workshop, addon.installed) }], removal === 'unsubscribe');
	}

	let choosingDestination = false;
	function chooseDestination() {
		if (!selectedCount) return;
		rememberFocus();
		choosingDestination = true;
	}

	function cancelDestination() {
		choosingDestination = false;
		restoreFocus();
	}

	async function extractSelected(destination) {
		const paths = selectedPaths;
		cancelDestination();
		try {
			const queued = await invoke('extract_installed_addons', { paths, destination });
			if (queued) taskMessage($_('bulk_extract_queued', { values: { count: queued } }));
			if (queued < paths.length) taskMessage($_('bulk_extract_skipped', { values: { count: paths.length - queued } }));
		} catch (error) {
			await message($_('bulk_extract_failed', { values: { error: String(error) } }), { kind: 'error' });
		}
	}

	async function copyLinks() {
		if (!linkIds.length) return;
		const ids = linkIds;
		const skipped = linklessCount;
		try {
			await navigator.clipboard.writeText(ids.map(id => 'https://steamcommunity.com/sharedfiles/filedetails/?id=' + id).join('\n'));
		} catch (error) {
			taskMessage($_('links_copy_failed', { values: { error: String(error) } }));
			return;
		}
		taskMessage($_('links_copied', { values: { count: ids.length } }));
		if (skipped) taskMessage($_('links_skipped', { values: { count: skipped } }));
	}

	function openLocations() {
		if (!folders.size || tooManyFolders) return;
		for (const path of folders.values()) invoke('open_file_location', { path });
	}

	function contextItems(addon) {
		if (!$selection.active || selectedCount < 2 || !$selection.items.has(addon.installed?.path)) return null;
		return [
			{ icon: FolderAdd, label: extractLabel, click: chooseDestination },
			{ icon: CircleMinus, label: unsubscribeLabel, click: () => removeSelected(true), disabled: !unsubscribePaths.length || !!$removalJob },
			{ icon: Trash2, label: deleteLabel, click: () => removeSelected(false), disabled: !deletePaths.length || !!$removalJob },
			{ divider: true },
			{ icon: LinkChain, label: $_('copy_links'), click: copyLinks, disabled: !linkIds.length },
			{ icon: Folder, label: $_('open_locations'), click: openLocations, disabled: !folders.size || tooManyFolders },
		];
	}
</script>

<svelte:window on:pointerdown={dismissFilters} on:keydown={onKeydown}/>

<section class="installed" bind:this={section}>
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
		<button type="button" class="select-mode" class:active={$selection.active} aria-pressed={$selection.active} on:click={toggleSelecting}>
			<SquareCheck size="1rem"/>
			{$_('select')}
		</button>
	</div>
	{#if $selection.active}
		<div class="selection-bar">
			<span class="count">{$_('selection_count', { values: { count: selectedCount, size: formatSize(selectedSize) } })}</span>
			<button type="button" on:click={selectAll}>{$_('select_all_count', { values: { count: total } })}</button>
			<button type="button" disabled={!selectedCount} on:click={selection.clear}>{$_('clear_selection')}</button>
			<span class="separator" aria-hidden="true"></span>
			<button type="button" disabled={!selectedCount} on:click={chooseDestination}><FolderAdd size="1rem"/>{$_('extract')}</button>
			<button type="button" bind:this={unsubscribeButton} disabled={!unsubscribePaths.length || !!$removalJob} on:click={() => removeSelected(true)}><CircleMinus size="1rem"/>{unsubscribeLabel}</button>
			<button type="button" disabled={!deletePaths.length || !!$removalJob} on:click={() => removeSelected(false)}><Trash2 size="1rem"/>{deleteLabel}</button>
			<button type="button" disabled={!linkIds.length} on:click={copyLinks}><LinkChain size="1rem"/>{$_('copy_links')}</button>
			<span use:tippy={tooManyFolders ? $_('open_locations_limit', { values: { max: MAX_LOCATIONS } }) : ''}>
				<button type="button" disabled={!folders.size || tooManyFolders} on:click={openLocations}><Folder size="1rem"/>{$_('open_locations')}</button>
			</span>
			<button type="button" class="done" on:click={selection.stop}>{$_('done')}</button>
			{#if blockedCount}<span class="note">{$_('bulk_cant_remove', { values: { count: blockedCount } })}</span>{/if}
		</div>
	{/if}
	<span class="sr-only" role="status">{$selection.active ? $_('selection_announce', { values: { count: selectedCount } }) : ''}</span>
	{#if bulkJob}
		<div class="job">
			<span>{$_(bulkJob.unsubscribe ? 'bulk_unsubscribing' : 'bulk_deleting', { values: { completed: bulkJob.completed, total: bulkJob.total } })}</span>
			<button type="button" on:click={stopRemoval}>{$_('stop')}</button>
		</div>
	{/if}
	{#if saveError}
		<p class="error" role="alert">{$_('installed_save_error', { values: { error: saveError } })}</p>
	{/if}
	{#if selectionError}
		<p class="error" role="alert">{selectionError}</p>
	{/if}
	{#if types.length === 0}
		<p class="status" role="status">{$_('installed_no_types')}</p>
	{:else}
		<div class="results">
			<AddonScroller bind:this={scroller} bind:total {next} {onClick} {selection} {contextItems} label={$_('installed_addons')} onSelectAll={selectAll} onDelete={deleteKey}/>
		</div>
	{/if}
</section>

<PreviewGMA active={previewingGMA} {promises} cancel={closePreview}/>
<DestinationSelect active={choosingDestination} cancel={cancelDestination} callback={extractSelected} text={extractLabel} forceCreateFolder={true}/>

<style>
	.installed {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}
	.toolbar, .selection-bar, .job {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: .75rem;
		padding: 1rem 1.5rem .5rem;
	}
	.selection-bar, .job {
		gap: .5rem;
		padding-top: .25rem;
	}
	.select-mode {
		margin-inline-start: auto;
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
	button:hover:not(:disabled), select:hover {
		background: var(--control-hover);
	}
	button:disabled {
		opacity: .45;
		cursor: default;
	}
	button.active, button.done {
		border-color: var(--neutral);
		background: var(--neutral-dark);
		color: #fff;
	}
	button.done:hover:not(:disabled) {
		background: var(--neutral);
	}
	button:focus-visible, select:focus-visible, input:focus-visible {
		outline: 2px solid var(--text);
		outline-offset: 2px;
	}
	.count {
		font-weight: 600;
		margin-inline-end: .25rem;
	}
	.separator {
		align-self: stretch;
		width: 1px;
		margin: 0 .25rem;
		background: var(--border);
	}
	.note {
		flex-basis: 100%;
		color: var(--text-muted);
		font-size: .85rem;
	}
	.job span {
		color: var(--text-soft);
	}
	.sr-only {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip-path: inset(50%);
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
