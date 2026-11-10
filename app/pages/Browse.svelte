<script>
	import { onMount, tick } from 'svelte';
	import { _ } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import Search from '@lucide/svelte/icons/search';
	import SlidersHorizontal from '@lucide/svelte/icons/sliders-horizontal';
	import LayoutGrid from '@lucide/svelte/icons/layout-grid';
	import List from '@lucide/svelte/icons/list';
	import Columns3 from '@lucide/svelte/icons/columns-3';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import SquareCheck from '@lucide/svelte/icons/square-check';
	import Check from '@lucide/svelte/icons/check';
	import X from '@lucide/svelte/icons/x';
	import CirclePlus from '@lucide/svelte/icons/circle-plus';
	import CircleMinus from '@lucide/svelte/icons/circle-minus';
	import CloudDownload from '@lucide/svelte/icons/cloud-download';
	import ListPlus from '@lucide/svelte/icons/list-plus';
	import LinkChain from '@lucide/svelte/icons/link';
	import LinkOut from '@lucide/svelte/icons/external-link';
	import Eye from '@lucide/svelte/icons/eye';
	import AddonScroller from '../components/AddonScroller.svelte';
	import BrowseFilters from '../components/BrowseFilters.svelte';
	import BrowseDetail from '../components/BrowseDetail.svelte';
	import { installedAddonsRevision } from '../steam';
	import { saveSettings } from '../settings.js';
	import { taskError, taskMessage } from '../transactions.js';
	import { playSound } from '../sounds.js';
	import { createSelection } from '../addon-selection.js';
	import { chooseCollections, workshopUrl } from '../collections.js';
	import {
		COLUMNS, KINDS, KIND_TAGS, TREND_DAYS, activeFilters, browseJob, browsePager, defaultQuery, downloadItems, installedIds, itemReference,
		loadSubscribed, normalizeQuery, sortsFor, stopJob, subscribeItems, subscribedIds, unsubscribeItems,
	} from '../browse.js';

	let query = defaultQuery();
	let view = AppSettings.browse_view ?? 'grid';
	let columns = [...(AppSettings.browse_columns ?? [])];
	let filtersOpen = AppSettings.browse_filters_open ?? true;
	let searchText = '';
	let searchTimer;
	let searchInput;
	let section;
	let scroller;
	let total = 0;
	let more = null;
	let pageStats = [];
	let loadedPages = [];
	let paused = false;
	let tagOptions = [];
	let stack = [];
	let columnsOpen = false;
	let columnsMenu;

	$: next = makePager(query);
	$: scanned = pageStats.reduce((count, page) => count + (page?.scanned ?? 0), 0);
	$: matching = pageStats.find(Boolean)?.total ?? 0;
	$: chips = activeFilters(query);
	$: sorts = sortsFor(query.source, query.text);
	$: opened = stack[stack.length - 1] ?? null;
	$: selectable = query.kind !== 'collection';
	$: extra = cardExtra($subscribedIds, $installedIds, view);

	onMount(loadSubscribed);

	let seenRevision = $installedAddonsRevision;
	$: if ($installedAddonsRevision !== seenRevision) {
		seenRevision = $installedAddonsRevision;
		loadSubscribed();
	}

	function makePager(query) {
		pageStats = [];
		loadedPages = [];
		paused = false;
		const pager = browsePager(query, {
			onPage: (page, result) => pager === next && pageLoaded(page, result),
			onPause: value => {
				if (pager === next) paused = value;
			},
		});
		return pager;
	}

	function pageLoaded(page, result) {
		if (page === 1) {
			pageStats = [];
			loadedPages = [];
		}
		pageStats[page] = { scanned: result.scanned, total: result.total };
		loadedPages[page] = result.items;
		if (query.kind !== 'addon' && query.kind !== 'collection') {
			const known = new Set(tagOptions);
			for (const item of result.items) for (const tag of item.workshop.tags) if (!KIND_TAGS.has(tag.toLowerCase())) known.add(tag);
			tagOptions = [...known].sort((a, b) => a.localeCompare(b));
		}
	}

	function update(patch) {
		const kindChanged = patch.kind && patch.kind !== query.kind;
		query = normalizeQuery({ ...query, ...patch });
		if (kindChanged) tagOptions = [];
		selection.stop();
	}

	function resetFilters() {
		const reset = defaultQuery();
		update({ ...reset, kind: query.kind, sort: query.sort, trendDays: query.trendDays, text: query.text });
	}

	function clearChip(chip) {
		update(chip.clear);
	}

	function applySearch() {
		clearTimeout(searchTimer);
		const reference = itemReference(searchText);
		if (reference) {
			openItem(reference);
			return;
		}
		if (searchText.trim() === query.text.trim()) return;
		const patch = { text: searchText };
		if (searchText.trim() && !query.text.trim() && query.source === 'everyone' && query.sort === 'popular') patch.sort = 'relevance';
		update(patch);
	}

	function searchKeydown(event) {
		if (event.key === 'Enter') {
			event.preventDefault();
			applySearch();
		} else if (event.key === 'Escape') {
			event.preventDefault();
			if (searchText) {
				searchText = '';
				applySearch();
			} else {
				scroller?.focusCurrent();
			}
		}
	}

	function onSearchInput() {
		clearTimeout(searchTimer);
		searchTimer = setTimeout(applySearch, 400);
	}

	function setView(value) {
		view = value;
		saveSettings({ browse_view: value }).catch(() => {});
	}

	function toggleColumn(column, checked) {
		columns = COLUMNS.filter(value => (value === column ? checked : columns.includes(value)));
		saveSettings({ browse_columns: columns }).catch(() => {});
	}

	function toggleFilters() {
		filtersOpen = !filtersOpen;
		saveSettings({ browse_filters_open: filtersOpen }).catch(() => {});
	}

	function dismissMenus(event) {
		if (columnsOpen && !columnsMenu?.contains(event.target)) columnsOpen = false;
	}

	const visible = () => !!section && section.offsetParent !== null;

	function onKeydown(event) {
		if (event.defaultPrevented || !visible()) return;
		if (columnsOpen && event.key === 'Escape') {
			columnsOpen = false;
			return;
		}
		if (opened) {
			if (event.key === 'Escape' && !event.target.closest?.('input, select, textarea, [role="dialog"]')) {
				event.preventDefault();
				back();
			}
			return;
		}
		if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 'f' && (event.target === document.body || section.contains(event.target))) {
			event.preventDefault();
			searchInput?.focus();
			searchInput?.select();
			return;
		}
		if (event.altKey || !section.contains(event.target) || event.target.closest('input, select, textarea')) return;
		if (event.key === 'Escape' && $selection.active) {
			event.preventDefault();
			selection.stop();
		} else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a' && selectable) {
			event.preventDefault();
			selectAll();
		}
	}

	function openItem(reference, title = null) {
		stack = [...stack, { reference: String(reference), title }];
	}

	function itemLoaded(index, workshop) {
		if (stack[index]) {
			stack[index] = { ...stack[index], title: workshop.title };
			stack = stack;
		}
	}

	async function back() {
		stack = stack.slice(0, -1);
		if (!stack.length) {
			await tick();
			scroller?.focusCurrent();
		}
	}

	function closeAll() {
		stack = [];
	}

	function showAuthor(author) {
		stack = [];
		searchText = '';
		update({ source: 'author', author, text: '', sort: 'newest' });
		if (!filtersOpen) toggleFilters();
	}

	async function onClick(event, workshop) {
		const item = await workshop;
		if (item) openItem(String(item.id), item.title);
	}

	function cardExtra(subscribed, installed, view) {
		return addon => {
			const id = String(addon.workshop.id);
			const badge = subscribed.has(id) ? $_('browse_badge_subscribed') : installed.has(id) ? $_('browse_badge_installed') : null;
			return view === 'grid' ? { badge, details: true } : { badge };
		};
	}

	const selection = createSelection();
	$: selectedItems = [...$selection.items.values()];
	$: selectedCount = selectedItems.length;
	$: toSubscribe = selectedItems.filter(item => !$subscribedIds.has(String(item.id)));
	$: toUnsubscribe = selectedItems.filter(item => $subscribedIds.has(String(item.id)));
	$: downloadable = selectedItems.filter(item => item.kind === 'addon');

	function selectAll() {
		selection.setAll(loadedPages.flat().filter(Boolean).map(addon => [addon.workshop.id, addon.workshop]));
	}

	function toggleSelecting() {
		if ($selection.active) selection.stop();
		else selection.start();
	}

	async function copyLinks(items) {
		try {
			await navigator.clipboard.writeText(items.map(item => workshopUrl(item.id)).join('\n'));
		} catch (error) {
			taskError($_('links_copy_failed', { values: { error: String(error) } }));
			return;
		}
		taskMessage($_('links_copied', { values: { count: items.length } }));
		playSound('success');
	}

	const ids = items => items.map(item => String(item.id));
	const subscribe = items => subscribeItems(ids(items));
	const unsubscribe = items => unsubscribeItems(items.map(item => ({ id: String(item.id), title: item.title })));

	function contextItems(addon) {
		const item = addon.workshop;
		if (selectable && $selection.active && selectedCount > 1 && $selection.items.has(item.id)) {
			return [
				{ icon: CirclePlus, label: $_('browse_subscribe_count', { values: { count: toSubscribe.length } }), click: () => subscribe(toSubscribe), disabled: !toSubscribe.length || !!$browseJob },
				{ icon: CircleMinus, label: $_('browse_unsubscribe_count', { values: { count: toUnsubscribe.length } }), click: () => unsubscribe(toUnsubscribe), disabled: !toUnsubscribe.length || !!$browseJob },
				{ icon: CloudDownload, label: $_('browse_download_count', { values: { count: downloadable.length } }), click: () => downloadItems(ids(downloadable)), disabled: !downloadable.length },
				{ icon: ListPlus, label: $_('collections_menu'), click: () => chooseCollections(ids(selectedItems)) },
				{ divider: true },
				{ icon: LinkChain, label: $_('copy_links'), click: () => copyLinks(selectedItems) },
			];
		}
		const subscribed = $subscribedIds.has(String(item.id));
		const menu = [{ icon: Eye, label: $_('open'), click: () => openItem(String(item.id), item.title) }];
		if (item.kind !== 'collection') {
			menu.push(subscribed
				? { icon: CircleMinus, label: $_('unsubscribe'), click: () => unsubscribe([item]), disabled: !!$browseJob }
				: { icon: CirclePlus, label: $_('browse_subscribe'), click: () => subscribe([item]), disabled: !!$browseJob });
			if (item.kind === 'addon') menu.push({ icon: CloudDownload, label: $_('download'), click: () => downloadItems([String(item.id)]) });
			menu.push({ icon: ListPlus, label: $_('collections_menu'), click: () => chooseCollections([String(item.id)]) });
		}
		menu.push({ divider: true });
		menu.push({ icon: LinkOut, label: $_('steam_workshop'), click: () => invoke('open_url', { url: workshopUrl(item.id) }).catch(error => taskError(String(error))) });
		menu.push({ icon: LinkChain, label: $_('copy_link'), click: () => copyLinks([item]) });
		return menu;
	}

	const columnLabel = column => $_('browse_column_' + column);
</script>

<svelte:window on:keydown={onKeydown} on:pointerdown={dismissMenus}/>

<section class="browse" bind:this={section}>
	<div class="toolbar">
		<div class="heading">
			{#if opened}
				<button type="button" class="plain crumb" on:click={closeAll}>{$_('browse_workshop')}</button>
				{#if stack.length > 1}
					<ChevronRight size="1rem"/>
					<button type="button" class="plain crumb" on:click={back}>{stack[stack.length - 2].title ?? stack[stack.length - 2].reference}</button>
				{/if}
				<ChevronRight size="1rem"/>
				<span class="current">{opened.title ?? opened.reference}</span>
			{:else}
				<span class="page-name">{$_('browse_workshop')}</span>
			{/if}
		</div>
		{#if !opened}
			<div class="controls">
				<div class="search">
					<Search size="1rem"/>
					<input type="search" bind:value={searchText} bind:this={searchInput} placeholder={$_('browse_search')} aria-label={$_('browse_search')} on:input={onSearchInput} on:keydown={searchKeydown}/>
				</div>
				<label class="sort">
					<span>{$_('workshop_sort')}</span>
					<select value={query.sort} disabled={sorts.length < 2} on:change={event => update({ sort: event.currentTarget.value })}>
						{#each sorts as sort}<option value={sort}>{$_('browse_sort_' + sort)}</option>{/each}
					</select>
				</label>
				{#if query.sort === 'popular'}
					<select value={query.trendDays} aria-label={$_('browse_period')} on:change={event => update({ trendDays: Number(event.currentTarget.value) })}>
						{#each TREND_DAYS as days}<option value={days}>{$_('browse_trend_' + days)}</option>{/each}
					</select>
				{/if}
				<button type="button" class:active={filtersOpen} aria-pressed={filtersOpen} on:click={toggleFilters}>
					<SlidersHorizontal size="1rem"/>{$_('workshop_filters')}{#if chips.length} ({chips.length}){/if}
				</button>
				<div class="view" role="group" aria-label={$_('browse_view')}>
					<button type="button" class="icon" class:active={view === 'grid'} aria-pressed={view === 'grid'} title={$_('browse_view_grid')} aria-label={$_('browse_view_grid')} on:click={() => setView('grid')}><LayoutGrid size="1rem"/></button>
					<button type="button" class="icon" class:active={view === 'list'} aria-pressed={view === 'list'} title={$_('browse_view_list')} aria-label={$_('browse_view_list')} on:click={() => setView('list')}><List size="1rem"/></button>
				</div>
				{#if view === 'list'}
					<div class="menu" bind:this={columnsMenu}>
						<button type="button" class="icon" aria-expanded={columnsOpen} aria-controls="browse-columns" title={$_('browse_columns')} aria-label={$_('browse_columns')} on:click={() => columnsOpen = !columnsOpen}><Columns3 size="1rem"/><ChevronDown size=".9rem"/></button>
						{#if columnsOpen}
							<fieldset id="browse-columns">
								<legend>{$_('browse_columns')}</legend>
								{#each COLUMNS as column}
									<label><input type="checkbox" checked={columns.includes(column)} on:change={event => toggleColumn(column, event.currentTarget.checked)}/>{columnLabel(column)}</label>
								{/each}
							</fieldset>
						{/if}
					</div>
				{/if}
				{#if selectable}
					<button type="button" class:active={$selection.active} on:click={toggleSelecting}>
						{#if $selection.active}<Check size="1rem"/>{$_('done')}{:else}<SquareCheck size="1rem"/>{$_('select')}{/if}
					</button>
				{/if}
			</div>
		{/if}
	</div>

	<div class="body">
		<div class="results-area" inert={!!opened} aria-hidden={!!opened}>
			<div class="modes" role="tablist" aria-label={$_('browse_mode')}>
				{#each KINDS as kind}
					<button type="button" role="tab" aria-selected={query.kind === kind} class:selected={query.kind === kind} on:click={() => update({ kind })}>{$_('browse_mode_' + kind)}</button>
				{/each}
			</div>
			{#if chips.length}
				<div class="chips">
					{#each chips as chip (chip.key)}
						<button type="button" class="chip" aria-label={$_('browse_remove_filter', { values: { filter: chip.label } })} on:click={() => clearChip(chip)}>{chip.label}<X size=".8rem"/></button>
					{/each}
					<button type="button" class="plain" on:click={resetFilters}>{$_('browse_clear_filters')}</button>
				</div>
			{/if}
			<p class="status" role="status">
				{#if pageStats.length}{$_('browse_scan_status', { values: { shown: total, scanned, total: matching } })}{/if}
				{#if query.source !== 'everyone'}<span class="limit">{$_('browse_source_limit')}</span>{/if}
			</p>
			{#if paused}
				<div class="panel">
					<span>{$_('browse_paused', { values: { scanned } })}</span>
					<button type="button" on:click={() => next.resume()}>{$_('browse_keep_scanning')}</button>
				</div>
			{/if}
			{#if selectable && $selection.active}
				<div class="panel">
					<div class="selection-summary">
						<span class="count">{$_('browse_selection_count', { values: { count: selectedCount } })}</span>
						<button type="button" class="plain" on:click={selectAll}>{$_('browse_select_loaded')}</button>
						<button type="button" class="plain" disabled={!selectedCount} on:click={selection.clear}>{$_('clear_selection')}</button>
					</div>
					<div class="selection-actions">
						<button type="button" disabled={!toSubscribe.length || !!$browseJob} on:click={() => subscribe(toSubscribe)}><CirclePlus size="1rem"/>{$_('browse_subscribe_count', { values: { count: toSubscribe.length } })}</button>
						<button type="button" disabled={!toUnsubscribe.length || !!$browseJob} on:click={() => unsubscribe(toUnsubscribe)}><CircleMinus size="1rem"/>{$_('browse_unsubscribe_count', { values: { count: toUnsubscribe.length } })}</button>
						<button type="button" disabled={!selectedCount} on:click={() => chooseCollections(ids(selectedItems))}><ListPlus size="1rem"/>{$_('collections_menu')}</button>
						{#if query.kind === 'addon'}
							<button type="button" disabled={!downloadable.length} on:click={() => downloadItems(ids(downloadable))}><CloudDownload size="1rem"/>{$_('browse_download_count', { values: { count: downloadable.length } })}</button>
						{/if}
					</div>
				</div>
			{/if}
			{#if $browseJob}
				<div class="panel job">
					<span id="browse-job-status">{$browseJob.retryIn ? $_('collections_save_throttled', { values: { seconds: $browseJob.retryIn } }) : $_($browseJob.action === 'subscribe' ? 'collections_subscribing' : 'bulk_unsubscribing', { values: { completed: $browseJob.completed, total: $browseJob.total } })}</span>
					<progress max={$browseJob.total} value={$browseJob.completed} aria-labelledby="browse-job-status"></progress>
					{#if $browseJob.stoppable}<button type="button" on:click={stopJob}>{$_('stop')}</button>{/if}
				</div>
			{/if}
			<div class="content">
				{#if filtersOpen}
					<BrowseFilters {query} {tagOptions} onChange={update}/>
				{/if}
				<div class="results">
					<AddonScroller bind:this={scroller} bind:total bind:more {next} {onClick} selection={selectable ? selection : null} {contextItems} {view} {columns} {columnLabel} {extra} label={$_('browse_workshop')} emptyMessage={$_('browse_no_results')} onSelectAll={selectable ? selectAll : null}/>
				</div>
			</div>
		</div>
		{#if opened}
			<div class="detail-area">
				{#key opened.reference + ':' + stack.length}
					<BrowseDetail reference={opened.reference} onOpen={reference => openItem(reference)} onAuthor={showAuthor} onLoaded={workshop => itemLoaded(stack.length - 1, workshop)}/>
				{/key}
			</div>
		{/if}
	</div>
</section>

<style>
	.browse {
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
	.heading {
		display: flex;
		align-items: center;
		gap: .4rem;
		margin-inline-end: auto;
		min-width: 0;
	}
	.page-name, .current {
		font-weight: 600;
	}
	.current {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.controls, .selection-summary, .selection-actions, .chips, .panel {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: .5rem;
	}
	.controls {
		gap: .75rem;
	}
	.body {
		position: relative;
		flex: 1;
		min-height: 0;
	}
	.results-area, .detail-area {
		position: absolute;
		inset: 0;
		display: flex;
		flex-direction: column;
		min-height: 0;
	}
	.detail-area {
		z-index: 2;
		background: var(--bg-content);
	}
	.content {
		display: flex;
		flex: 1;
		min-height: 0;
		padding-top: .5rem;
	}
	.results {
		flex: 1;
		min-width: 0;
		min-height: 0;
	}
	.modes {
		display: flex;
		flex-wrap: wrap;
		gap: 1rem;
		padding: 0 1.5rem;
		border-bottom: 1px solid var(--border);
	}
	.modes button {
		padding: .5rem .1rem;
		border: 0;
		border-bottom: 2px solid transparent;
		border-radius: 0;
		background: none;
		color: var(--text-soft);
	}
	.modes button:hover:not(:disabled) {
		background: none;
		color: var(--text);
	}
	.modes button.selected {
		border-bottom-color: var(--neutral);
		color: var(--text);
		font-weight: 600;
	}
	.chips {
		padding: .6rem 1.5rem 0;
	}
	.chip {
		padding: .25rem .5rem;
		gap: .3rem;
		font-size: .85rem;
	}
	.status {
		display: flex;
		flex-wrap: wrap;
		gap: .25rem 1rem;
		min-height: 1.2em;
		margin: .5rem 1.5rem 0;
		color: var(--text-soft);
	}
	.limit {
		color: var(--text-muted);
	}
	.panel {
		gap: .5rem 1rem;
		margin: .5rem 1.5rem 0;
		padding: .5rem;
		padding-inline-start: 1rem;
		border: 1px solid var(--border);
		border-radius: 4px;
		background: var(--bg-panel);
	}
	.selection-actions {
		margin-inline-start: auto;
	}
	.panel button {
		padding: .45rem .7rem;
	}
	.count {
		font-weight: 600;
	}
	.job span {
		color: var(--text-soft);
	}
	progress {
		flex: 1;
		min-width: 8rem;
		height: .5rem;
		accent-color: var(--neutral);
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
	button.icon {
		padding: .6rem;
		gap: .2rem;
	}
	button:hover:not(:disabled), select:hover:not(:disabled) {
		background: var(--control-hover);
	}
	button:disabled, select:disabled {
		opacity: .45;
		cursor: default;
	}
	button.active {
		border-color: var(--neutral);
		background: var(--neutral-dark);
		color: #fff;
	}
	button.active:hover:not(:disabled) {
		background: var(--neutral);
	}
	button.plain {
		border-color: transparent;
		background: none;
		color: var(--link);
	}
	button.plain:hover:not(:disabled) {
		background: var(--control);
	}
	button.crumb {
		padding-inline: .25rem;
		color: var(--text);
	}
	button:focus-visible, select:focus-visible, input:focus-visible {
		outline: 2px solid var(--text);
		outline-offset: 2px;
	}
	.view {
		display: inline-flex;
		gap: .25rem;
	}
	.sort {
		display: flex;
		align-items: center;
		gap: .5rem;
	}
	.search {
		display: flex;
		align-items: center;
		gap: .5rem;
		padding-inline-start: .6rem;
		border: 1px solid var(--border-strong);
		border-radius: 4px;
		background: var(--control);
		color: var(--text-muted);
	}
	.search input {
		min-width: 12rem;
		padding: .6rem .75rem .6rem 0;
		border: 0;
		background: none;
		color: var(--text);
		font: inherit;
	}
	.search input:focus-visible {
		outline: none;
	}
	.search:focus-within {
		outline: 2px solid var(--text);
		outline-offset: 2px;
	}
	.menu {
		position: relative;
	}
	fieldset {
		position: absolute;
		top: calc(100% + .5rem);
		inset-inline-end: 0;
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
		padding: .4rem .25rem;
		cursor: pointer;
	}
	input[type='checkbox'] {
		accent-color: var(--neutral);
	}
</style>
