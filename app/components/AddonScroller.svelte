<script>
	import Addon from "./Addon.svelte";
	import AddonRow from "./AddonRow.svelte";
	import AddonPlaceholders from "./AddonPlaceholders.svelte";
	import { _ } from "svelte-i18n";
	import { tick } from "svelte";
	import { fade } from "svelte/transition";
	import { flip } from "svelte/animate";
	import Dead from "./Dead.svelte";
	import Loading from "./Loading.svelte";
	import { addonName } from "../addon-removal.js";
	import { moveIndex } from "../addon-selection.js";

	export let next;
	export let onClick = null;
	export let onNewAddonClick = null;
	export let selection = null;
	export let label = null;
	export let contextItems = null;
	export let onSelectAll = null;
	export let onDelete = null;
	export let emptyMessage = null;
	export let total = 0;
	export let view = 'grid';
	export let columns = [];
	export let columnLabel = null;
	export let extra = null;
	export let more = null;
	export let quickSubscribe = false;
	export let onQuickLook = null;
	export let placeholders = false;

	const NUMERIC_COLUMNS = ['size', 'subscribers', 'favorites', 'rating'];

	// Removing more cards than this updates the grid at once instead of animating each card out.
	const ANIMATED_REMOVALS = 20;

	let loading = true;
	let loaded = false;
	let error = null;
	let addons = [];
	let pages = 0;
	let request = 0;
	let animateChanges = true;

	const key = addon => addon.installed?.path ?? addon.workshop?.id;

	$: rowTemplate = `2.5rem minmax(12rem, 3fr) ${columns.map(column => (column === 'tags' || column === 'author' ? 'minmax(7rem, 1.5fr)' : 'minmax(5.5rem, 1fr)')).join(' ')}${quickSubscribe ? ' 1.75rem' : ''}`;

	// Cards that stay keep their component, so their previews do not reload.
	function replaceAddons(replacement, animate) {
		const kept = new Set(replacement.map(key));
		const removed = addons.filter(addon => !kept.has(key(addon))).length;
		animateChanges = animate && removed <= ANIMATED_REMOVALS;
		addons = replacement;
	}

	// A new `next` replaces the list once its first page arrives, so the current cards stay on screen.
	function fetchNext(reset = false) {
		const id = reset ? ++request : request;
		const page = reset ? 1 : pages + 1;
		loading = true;

		next(page).then(([fetchedTotal, fetchedAddons, fetchedMore]) => {
			if (id !== request) return;
			const hadFocus = !!listbox?.contains(document.activeElement);
			const focusedIndex = indexOf(focusKey);
			const base = reset ? [] : addons;
			const seen = new Set(base.map(key));
			const merged = base.concat(fetchedAddons.filter(addon => !seen.has(key(addon))));
			if (reset) replaceAddons(merged, false);
			else addons = merged;
			pages = page;
			total = reset || fetchedMore == null ? fetchedTotal : total + fetchedTotal;
			more = fetchedMore ?? null;
			if (reset && scroller) scroller.scrollTop = 0;
			error = null;
			loaded = true;
			loading = false;
			if (reset && hadFocus) restoreFocus(focusedIndex);
			if (more) tick().then(infiniScroll);
		}, reason => {
			if (id !== request) return;
			if (reset) {
				replaceAddons([], false);
				pages = 0;
				more = null;
				error = reason;
				loaded = true;
			}
			loading = false;
		});
	}
	$: next, fetchNext(true);

	/** Reloads the pages already shown and swaps them in place, keeping the scroll position and focus. */
	export async function refresh() {
		if (!loaded || !pages) return fetchNext(true);
		const id = request;
		let count;
		let fetched;
		try {
			do {
				count = pages;
				fetched = await Promise.all(Array.from({ length: count }, (item, index) => next(index + 1)));
				if (id !== request) return;
			} while (count !== pages);
		} catch (reason) {
			if (id === request) console.error(reason);
			return;
		}
		const hadFocus = !!listbox?.contains(document.activeElement);
		const focusedIndex = indexOf(focusKey);
		const seen = new Set();
		replaceAddons(fetched.flatMap(([, page]) => page).filter(addon => !seen.has(key(addon)) && seen.add(key(addon))), true);
		const last = fetched[fetched.length - 1];
		more = last[2] ?? null;
		total = more == null ? last[0] : fetched.reduce((count, [pageTotal]) => count + pageTotal, 0);
		error = null;
		if (hadFocus) restoreFocus(focusedIndex);
	}

	/** Drops the cards of removed addons right away, before the list reloads. */
	export function remove(keys) {
		const gone = new Set(keys);
		if (!addons.some(addon => gone.has(key(addon)))) return;
		const hadFocus = !!listbox?.contains(document.activeElement);
		const focusedIndex = indexOf(focusKey);
		const remaining = addons.filter(addon => !gone.has(key(addon)));
		if (gone.has(focusKey)) focusKey = remaining.length ? key(remaining[Math.min(Math.max(focusedIndex, 0), remaining.length - 1)]) : null;
		total = Math.max(total - (addons.length - remaining.length), 0);
		replaceAddons(remaining, true);
		if (hadFocus) restoreFocus(focusedIndex);
	}

	let scroller;
	const hasMore = () => (more == null ? addons.length < total : more);

	function infiniScroll() {
		if (!scroller || loading) return;
		if (scroller.offsetHeight + scroller.scrollTop >= scroller.scrollHeight * .9 && hasMore()) {
			fetchNext();
		}
	}

	let listbox;
	let focusKey = null;
	let anchorKey = null;
	let openRequested = false;

	const indexOf = path => (path == null ? -1 : addons.findIndex(addon => key(addon) === path));
	$: focusIndex = Math.max(addons.findIndex(addon => key(addon) === focusKey), 0);
	$: selecting = !!$selection?.active;
	$: selected = $selection?.items;
	$: if (!selecting) anchorKey = null;

	// Cards fading out stay in the DOM as inert elements until their transition ends.
	function options() {
		return listbox ? [...listbox.children].filter(option => !option.inert) : [];
	}

	function optionAt(index) {
		return options()[index];
	}

	function focusOption(index) {
		const option = optionAt(index);
		if (!option) return;
		focusKey = key(addons[index]);
		option.focus({ preventScroll: true });
		option.scrollIntoView({ block: 'nearest' });
	}

	async function restoreFocus(previousIndex) {
		await tick();
		if (!addons.length || !listbox || listbox.contains(document.activeElement)) return;
		const index = indexOf(focusKey);
		focusOption(index >= 0 ? index : Math.min(Math.max(previousIndex, 0), addons.length - 1));
	}

	function addRange(fromKey, toIndex) {
		const fromIndex = indexOf(fromKey);
		const start = Math.min(fromIndex >= 0 ? fromIndex : toIndex, toIndex);
		const end = Math.max(fromIndex >= 0 ? fromIndex : toIndex, toIndex);
		selection.add(addons.slice(start, end + 1).map(addon => [key(addon), addon.installed ?? addon.workshop]));
	}

	function toggle(index) {
		const addon = addons[index];
		anchorKey = key(addon);
		selection.toggle(anchorKey, addon.installed ?? addon.workshop);
	}

	function cardClick(event, index, workshop, installed) {
		if (openRequested || !selection) return onClick?.(event, workshop, installed);
		if (event.shiftKey) {
			anchorKey ??= focusKey ?? key(addons[index]);
			addRange(anchorKey, index);
		} else if (selecting || event.ctrlKey || event.metaKey || event.target.closest?.('[data-select-toggle]')) {
			toggle(index);
		} else {
			return onClick?.(event, workshop, installed);
		}
		focusKey = key(addons[index]);
	}

	function cardMousedown(event) {
		if (!event.shiftKey || event.button !== 0) return;
		event.preventDefault();
		event.currentTarget.focus({ preventScroll: true });
	}

	function openFocused() {
		openRequested = true;
		try {
			optionAt(focusIndex)?.firstElementChild?.click();
		} finally {
			openRequested = false;
		}
	}

	function gridMetrics() {
		const style = getComputedStyle(listbox);
		const columns = Math.max(style.gridTemplateColumns.split(' ').length, 1);
		const rowHeight = (optionAt(0)?.offsetHeight ?? 1) + (parseFloat(style.rowGap) || 0);
		return { columns, pageRows: Math.max(Math.floor(scroller.clientHeight / rowHeight), 1), rtl: style.direction === 'rtl' };
	}

	function onKeydown(event) {
		if (!addons.length || event.altKey || (event.target !== listbox && event.target.getAttribute('role') !== 'option')) return;
		const index = focusIndex;
		if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
			event.preventDefault();
			onSelectAll?.();
			return;
		}
		switch (event.key) {
			case ' ':
				event.preventDefault();
				if (onQuickLook && !selecting && !event.shiftKey) {
					if (!event.repeat) onQuickLook(addons[index]);
				} else if (event.shiftKey && anchorKey) addRange(anchorKey, index);
				else toggle(index);
				return;
			case 'Enter':
				event.preventDefault();
				openFocused();
				return;
			case 'Escape':
				if (!selecting) return;
				event.preventDefault();
				selection.stop();
				return;
			case 'Delete':
				event.preventDefault();
				onDelete?.(addons[index]);
				return;
		}
		const { columns, pageRows, rtl } = gridMetrics();
		const target = moveIndex(event.key, index, addons.length, columns, pageRows, rtl);
		if (target === null) return;
		event.preventDefault();
		if (event.shiftKey) {
			anchorKey ??= key(addons[index]);
			addRange(anchorKey, target);
		}
		focusOption(target);
		if (target >= addons.length - columns && hasMore() && !loading) fetchNext();
	}

	function onFocusin(event) {
		const index = options().indexOf(event.target);
		if (addons[index]) focusKey = key(addons[index]);
	}

	export function focusCurrent() {
		focusOption(focusIndex);
	}

	function keyboardMenu(event) {
		if (event.target !== event.currentTarget) return;
		event.preventDefault();
		event.stopPropagation();
		const card = event.currentTarget.firstElementChild;
		const rect = card.getBoundingClientRect();
		card.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: rect.left + rect.width / 2, clientY: rect.top + rect.height / 2 }));
	}
</script>

<main class="scrollbar" on:scroll={infiniScroll} bind:this={scroller}>
	{#if more == null && addons.length !== 0 && total !== 0 && addons.length !== total}
		<p id="amount">{$_('showing_num_addons', { values: { num: addons.length.toLocaleString(), total: total.toLocaleString() }})}</p>
	{/if}

	{#if onNewAddonClick}
		<div id="grid">
			<Addon newAddon={true} onClick={onNewAddonClick}/>
			{#each addons as addon (key(addon))}
				<Addon {onClick} workshopData={addon.workshop ? Promise.resolve(addon.workshop) : null} installedData={addon.installed ? Promise.resolve(addon.installed) : null}/>
			{/each}
			{#if !loaded}
				<Loading size="2rem"/>
			{:else if error}
				<Dead size="2rem"/>
				<div id="error">{JSON.stringify(error)}</div>
			{/if}
		</div>
	{:else if placeholders && !error && (!loaded || (addons.length === 0 && more))}
		<div id="grid" class:list={view === 'list'}><AddonPlaceholders {view} template={rowTemplate}/></div>
	{:else if !loaded}
		<Loading size="2rem"/>
	{:else if error}
		<Dead size="2rem"/>
		<div id="error">{JSON.stringify(error)}</div>
	{:else if addons.length === 0 && more}
		<Loading size="2rem"/>
	{:else if addons.length === 0}
		<Dead size="2rem"/>
		<div id="error">{emptyMessage ?? $_('ERR_NO_ADDONS_FOUND')}</div>
	{:else if selection}
		{#if view === 'list'}
			<div class="list-head" style:grid-template-columns={rowTemplate} aria-hidden="true">
				<span></span><span>{$_('browse_column_title')}</span>
				{#each columns as column}<span class:end={NUMERIC_COLUMNS.includes(column)}>{columnLabel?.(column) ?? column}</span>{/each}
				{#if quickSubscribe}<span></span>{/if}
			</div>
		{/if}
		<div id="grid" class:list={view === 'list'} role="listbox" aria-multiselectable="true" aria-label={label} tabindex="-1" bind:this={listbox} on:keydown={onKeydown} on:focusin={onFocusin}>
			{#each addons as addon, index (key(addon))}
				<div class="option" role="option" aria-selected={!!selected?.has(key(addon))} aria-label={addonName(addon.workshop, addon.installed)} aria-setsize={total} aria-posinset={index + 1} tabindex={index === focusIndex ? 0 : -1} on:mousedown={cardMousedown} on:contextmenu={keyboardMenu} animate:flip={{ duration: animateChanges ? 200 : 0 }} out:fade={{ duration: animateChanges ? 150 : 0 }}>
					{#if view === 'list'}
						<AddonRow onClick={(event, workshop, installed) => cardClick(event, index, workshop, installed)} contextItems={contextItems && (() => contextItems(addon))} {selecting} selected={!!selected?.has(key(addon))} workshop={addon.workshop} {columns} template={rowTemplate} {quickSubscribe} {...(extra?.(addon) ?? {})}/>
					{:else}
						<Addon onClick={(event, workshop, installed) => cardClick(event, index, workshop, installed)} contextItems={contextItems && (() => contextItems(addon))} selectable={true} {selecting} selected={!!selected?.has(key(addon))} workshopData={addon.workshop ? Promise.resolve(addon.workshop) : null} installedData={addon.installed ? Promise.resolve(addon.installed) : null} {quickSubscribe} {...(extra?.(addon) ?? {})}/>
					{/if}
				</div>
			{/each}
			{#if placeholders && more && loading}<AddonPlaceholders {view} count={view === 'list' ? 3 : 6} template={rowTemplate}/>{/if}
		</div>
		{#if more && loading && !placeholders}<div class="more"><Loading size="1.5rem"/></div>{/if}
	{:else if view === 'list'}
		<div class="list-head" style:grid-template-columns={rowTemplate} aria-hidden="true">
			<span></span><span>{$_('browse_column_title')}</span>
			{#each columns as column}<span class:end={NUMERIC_COLUMNS.includes(column)}>{columnLabel?.(column) ?? column}</span>{/each}
		</div>
		<div id="grid" class="list">
			{#each addons as addon (key(addon))}
				<AddonRow {onClick} contextItems={contextItems && (() => contextItems(addon))} workshop={addon.workshop} {columns} template={rowTemplate} {quickSubscribe} {...(extra?.(addon) ?? {})}/>
			{/each}
			{#if placeholders && more && loading}<AddonPlaceholders {view} count={view === 'list' ? 3 : 6} template={rowTemplate}/>{/if}
		</div>
		{#if more && loading && !placeholders}<div class="more"><Loading size="1.5rem"/></div>{/if}
	{:else}
		<div id="grid">
			{#each addons as addon (key(addon))}
				<Addon {onClick} workshopData={addon.workshop ? Promise.resolve(addon.workshop) : null} installedData={addon.installed ? Promise.resolve(addon.installed) : null}/>
			{/each}
			{#if placeholders && more && loading}<AddonPlaceholders {view} count={view === 'list' ? 3 : 6} template={rowTemplate}/>{/if}
		</div>
	{/if}
</main>

<style>
	main {
		flex: 1;
		overflow: auto;
		position: relative;
		padding: 1.5rem;
		height: 100%;
	}
	#amount {
		margin: 0 0 1rem;
		color: var(--text-soft);
	}
	main #grid {
		position: relative;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(var(--addon-card-size, 200px), 1fr));
		grid-auto-flow: dense;
		align-items: center;
		grid-gap: 1rem;
	}
	#grid:focus {
		outline: none;
	}
	.option {
		height: 100%;
	}
	#grid.list {
		grid-template-columns: minmax(0, 1fr);
		grid-gap: 0;
	}
	.list-head {
		position: sticky;
		top: -1.5rem;
		z-index: 1;
		display: grid;
		gap: .75rem;
		padding: .5rem .75rem;
		border-bottom: 1px solid var(--border);
		background: var(--bg-content);
		color: var(--text-muted);
		font-size: .8rem;
		font-weight: 600;
	}
	.list-head .end {
		text-align: end;
	}
	.more {
		position: relative;
		height: 3rem;
	}
	.option:focus {
		outline: none;
	}
	.option:focus-visible > :global(.addon > #card), .option:focus-visible > :global(.addon-row) {
		outline: 2px solid #127cff;
		outline-offset: 2px;
	}
	main > :global(svg) {
		position: absolute;
		top: 1.5rem !important;
		bottom: auto !important;
	}
	main > :global(.loading) {
		opacity: .2;
	}
	#error {
		position: absolute;
		margin: auto;
		top: 4.5rem;
		text-align: center;
		padding: 1.5rem;
		left: 0;
		right: 0;
		padding-top: 0;
		padding-bottom: 0;
	}
</style>
