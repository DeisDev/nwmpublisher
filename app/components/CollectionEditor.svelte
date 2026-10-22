<script>
	import { _, locale } from 'svelte-i18n';
	import { Channel, invoke } from '@tauri-apps/api/core';
	import * as dialog from '@tauri-apps/plugin-dialog';
	import { createEventDispatcher, onDestroy } from 'svelte';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import BellPlus from '@lucide/svelte/icons/bell-plus';
	import CloudDownload from '@lucide/svelte/icons/cloud-download';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Search from '@lucide/svelte/icons/search';
	import Plus from '@lucide/svelte/icons/plus';
	import X from '@lucide/svelte/icons/x';
	import ImagePlus from '@lucide/svelte/icons/image-plus';
	import ListMinus from '@lucide/svelte/icons/list-minus';
	import Loading from './Loading.svelte';
	import Timestamp from './Timestamp.svelte';
	import BBCodeEditor from './BBCodeEditor.svelte';
	import CollectionItemRow from './CollectionItemRow.svelte';
	import { Transaction, taskMessage } from '../transactions.js';
	import { formatSize } from '../format.js';
	import { playSound } from '../sounds';
	import { collections, compareTitles, loadCollections, workshopUrl } from '../collections.js';

	export let collection = null;
	export let others = [];
	export let notice = null;

	const dispatch = createEventDispatcher();
	const visibilityOptions = ['public', 'friends_only', 'private', 'unlisted'];
	const itemSortOptions = ['order', 'title', 'size', 'updated'];
	const titleMaxBytes = 128;
	const descriptionMaxBytes = 7999;
	const encoder = new TextEncoder();

	const creating = !collection;
	const id = collection?.id ?? null;
	let original = collection?.fields ?? { title: '', description: '', visibility: 'private', items: [] };
	let title = original.title;
	let description = original.description;
	let visibility = original.visibility;
	let items = [...original.items];
	let previewUrl = collection?.previewUrl ?? null;
	let preview = null;
	let historyKey = 0;
	let titleTouched = false;
	let tab = creating ? 'details' : 'items';
	let saving = false;
	let reloading = false;
	let error = notice;
	let needsReload = false;
	let saved = false;
	let destroyed = false;
	onDestroy(() => { destroyed = true; searchId++; });

	let entries = new Map();
	let entriesLoading = 0;
	let entriesError = null;
	const requested = new Set();

	async function loadEntries(ids) {
		const missing = [...new Set(ids)].filter(item => !entries.has(item) && !requested.has(item));
		if (!missing.length) return;
		missing.forEach(item => requested.add(item));
		entriesLoading++;
		entriesError = null;
		try {
			const lookup = await invoke('collection_entries', { references: missing });
			if (destroyed) return;
			const next = new Map(entries);
			for (const entry of lookup.entries) next.set(entry.id, entry);
			entries = next;
		} catch (err) {
			if (!destroyed) entriesError = String(err);
		} finally {
			missing.forEach(item => requested.delete(item));
			entriesLoading--;
		}
	}
	if (!creating) loadEntries(items);

	function findMissing(items, entries) {
		const present = new Set(items);
		const missing = new Map();
		for (const item of items) {
			for (const dependency of entries.get(item)?.dependencies ?? []) {
				if (present.has(dependency)) continue;
				if (!missing.has(dependency)) missing.set(dependency, []);
				missing.get(dependency).push(item);
			}
		}
		return missing;
	}

	$: originalSet = new Set(original.items);
	$: itemSet = new Set(items);
	$: additions = items.filter(item => !originalSet.has(item));
	$: removals = original.items.filter(item => !itemSet.has(item));
	$: detailsChanged = title !== original.title || description !== original.description || visibility !== original.visibility || !!preview;
	$: dirty = creating || additions.length > 0 || removals.length > 0 || detailsChanged;
	$: titleBytes = encoder.encode(title).length;
	$: descriptionBytes = encoder.encode(description).length;
	$: titleError = !title.trim() ? $_('collections_title_required') : titleBytes > titleMaxBytes ? $_('collections_title_too_long', { values: { max: titleMaxBytes } }) : null;
	$: descriptionError = descriptionBytes > descriptionMaxBytes ? $_('collections_description_too_long', { values: { max: descriptionMaxBytes } }) : null;
	$: invalid = !!titleError || !!descriptionError;
	$: locked = saving || reloading || needsReload;
	$: numberLocale = $locale === 'kr' ? 'ko' : ($locale ?? 'en');
	$: totalSize = items.reduce((size, item) => size + (entries.get(item)?.fileSize ?? 0), 0);
	$: missingRequirements = findMissing(items, entries);
	$: loadEntries([...missingRequirements.keys()]);
	$: withStatus = status => items.filter(item => entries.get(item)?.status === status);
	$: unavailable = withStatus('unavailable');
	$: banned = withStatus('banned');
	$: unsupported = withStatus('unsupported');
	$: duplicates = new Map(items.map(item => [item, others.filter(other => other.fields.items.includes(item))]).filter(([, list]) => list.length));
	$: problemCount = unavailable.length + banned.length + unsupported.length + missingRequirements.size;
	$: otherNames = item => duplicates.get(item)?.map(other => other.fields.title).join(', ') ?? '';

	let itemQuery = '';
	let itemSort = 'order';
	let selectedItems = new Set();
	$: shownItems = sortItems(items, entries, itemQuery, itemSort);

	function sortItems(items, entries, query, sort) {
		const term = query.trim().toLocaleLowerCase();
		const list = items.filter(item => !term || item === term || (entries.get(item)?.title ?? '').toLocaleLowerCase().includes(term));
		if (sort === 'title') list.sort((a, b) => compareTitles(entries.get(a)?.title ?? a, entries.get(b)?.title ?? b));
		else if (sort === 'size') list.sort((a, b) => (entries.get(b)?.fileSize ?? 0) - (entries.get(a)?.fileSize ?? 0));
		else if (sort === 'updated') list.sort((a, b) => (entries.get(b)?.timeUpdated ?? 0) - (entries.get(a)?.timeUpdated ?? 0));
		return list;
	}

	function toggleItem(item, checked) {
		const next = new Set(selectedItems);
		if (checked) next.add(item);
		else next.delete(item);
		selectedItems = next;
	}

	function addItems(list) {
		const fresh = list.filter(entry => entry.status === 'available' && entry.id !== id && !itemSet.has(entry.id));
		if (!fresh.length) return 0;
		const next = new Map(entries);
		for (const entry of fresh) next.set(entry.id, entry);
		entries = next;
		items = [...items, ...fresh.map(entry => entry.id)];
		saved = false;
		return fresh.length;
	}

	function removeItems(list) {
		const remove = new Set(list);
		items = items.filter(item => !remove.has(item));
		selectedItems = new Set([...selectedItems].filter(item => !remove.has(item)));
		saved = false;
	}

	function restoreItem(item) {
		items = [...items, item];
	}

	let query = '';
	let searchedQuery = '';
	let results = [];
	let total = 0;
	let page = 0;
	let searching = false;
	let searchError = null;
	let searched = false;
	let searchId = 0;
	let adding = new Set();

	function onSearchInput() {
		searchId++;
		searching = false;
		searchError = null;
		searched = false;
		results = [];
	}

	async function search(more = false) {
		if (locked || searching || !query.trim()) return;
		const request = ++searchId;
		const term = more ? searchedQuery : query.trim();
		const nextPage = more ? page + 1 : 1;
		searching = true;
		searchError = null;
		if (!more) { results = []; searched = false; }
		try {
			const response = await invoke('search_required_addons', { query: term, page: nextPage });
			if (request !== searchId) return;
			results = more ? [...results, ...response.items.filter(entry => !results.some(current => current.id === entry.id))] : response.items;
			total = response.total;
			page = nextPage;
			searchedQuery = term;
			searched = true;
		} catch (err) {
			if (request === searchId) searchError = String(err);
		} finally {
			if (request === searchId) searching = false;
		}
	}

	async function addResult(result) {
		adding = new Set(adding).add(result.id);
		try {
			const lookup = await invoke('collection_entries', { references: [result.id] });
			if (!addItems(lookup.entries)) searchError = $_('collections_cannot_add', { values: { title: result.title ?? result.id } });
		} catch (err) {
			searchError = String(err);
		} finally {
			const next = new Set(adding);
			next.delete(result.id);
			adding = next;
		}
	}

	let pasteText = '';
	let pasting = false;
	let pasteMessage = null;
	let pasteError = null;

	async function addPasted() {
		const references = pasteText.split(/[\s,]+/).filter(Boolean);
		if (locked || pasting || !references.length) return;
		pasting = true;
		pasteMessage = null;
		pasteError = null;
		try {
			const lookup = await invoke('collection_entries', { references });
			const added = addItems(lookup.entries);
			pasteMessage = $_('collections_paste_result', { values: { added, skipped: references.length - added } });
			if (added) pasteText = '';
		} catch (err) {
			pasteError = String(err);
		} finally {
			pasting = false;
		}
	}

	function chooseImage() {
		dialog.open({ filters: [{ extensions: ['jpg', 'jpeg', 'png', 'gif'], name: $_('collections_image_pick') }] }).then(path => {
			if (!path || destroyed) return;
			invoke('verify_icon', { path }).then(([dataUrl]) => {
				if (destroyed) return;
				preview = { path, dataUrl };
				saved = false;
			}, transactionId => new Transaction(transactionId, () => ''));
		});
	}

	function discard() {
		title = original.title;
		description = original.description;
		visibility = original.visibility;
		items = [...original.items];
		preview = null;
		selectedItems = new Set();
		historyKey++;
	}

	async function save() {
		if (locked || !dirty || invalid) return;
		saving = true;
		error = null;
		saved = false;
		try {
			if (creating) {
				const created = await invoke('create_collection', { draft: { title, description, visibility, preview: preview?.path ?? null } });
				playSound('success');
				dispatch('created', created);
				return;
			}
			const result = await invoke('save_collection', { request: {
				collectionId: id,
				original,
				desired: { title, description, visibility, items },
				preview: preview?.path ?? null,
			} });
			original = result.saved;
			if (result.previewSaved) {
				previewUrl = preview.dataUrl;
				preview = null;
			}
			if (result.error) {
				error = result.error;
				needsReload = true;
			} else {
				title = original.title;
				description = original.description;
				visibility = original.visibility;
				items = [...original.items];
				saved = true;
			}
			dispatch('saved');
		} catch (err) {
			error = String(err);
			needsReload = !creating;
		} finally {
			saving = false;
		}
	}

	async function reload() {
		if (creating || saving || reloading) return;
		reloading = true;
		error = null;
		await loadCollections();
		reloading = false;
		if (destroyed) return;
		const latest = $collections.items.find(other => other.id === id);
		if ($collections.error) error = $collections.error;
		else if (!latest) error = $_('collections_missing');
		else rebase(latest);
	}

	function rebase(latest) {
		const pendingAdditions = additions;
		const pendingRemovals = new Set(removals);
		const keepTitle = title !== original.title;
		const keepDescription = description !== original.description;
		const keepVisibility = visibility !== original.visibility;
		original = latest.fields;
		previewUrl = latest.previewUrl;
		items = [...original.items.filter(item => !pendingRemovals.has(item)), ...pendingAdditions.filter(item => !original.items.includes(item))];
		if (!keepTitle) title = original.title;
		if (!keepDescription) { description = original.description; historyKey++; }
		if (!keepVisibility) visibility = original.visibility;
		needsReload = false;
		loadEntries(items);
	}

	async function back() {
		if (saving) return;
		const unsaved = creating ? !!(title || description || preview) : dirty;
		if (unsaved && !await dialog.confirm($_('collections_discard_confirm'), { title: $_('collections'), kind: 'warning' })) return;
		dispatch('close');
	}

	let subscribing = null;
	let subscribeResult = null;
	let subscribeError = null;

	async function subscribeAll() {
		if (subscribing) return;
		subscribing = { completed: 0, total: 0 };
		subscribeResult = null;
		subscribeError = null;
		try {
			subscribeResult = await invoke('subscribe_collection', { collectionId: id, onProgress: new Channel(update => { if (subscribing) subscribing = update; }) });
		} catch (err) {
			subscribeError = String(err);
		} finally {
			subscribing = null;
		}
	}

	function downloadAll() {
		invoke('workshop_download', { ids: [Number(id)] });
		playSound('success');
		taskMessage($_('sent_to_downloader'));
	}
</script>

<section class="editor-view">
	<header>
		<button type="button" class="icon-button" on:click={back} aria-label={$_('collections_back')} title={$_('collections_back')}><ArrowLeft size="1rem"/></button>
		{#if preview || previewUrl}<img src={preview?.dataUrl ?? previewUrl} alt=""/>{:else}<div class="image-placeholder" aria-hidden="true">?</div>{/if}
		<div class="heading">
			<h2>{creating ? $_('collections_new') : original.title}</h2>
			{#if !creating}
				<p>
					{$_('workshop_visibility_' + original.visibility)} · {$_('collections_item_count', { values: { count: original.items.length } })}{#if totalSize} · {formatSize(totalSize)}{/if}
					· {$_('collections_subscribers', { values: { count: (collection.subscriptions ?? 0).toLocaleString(numberLocale) } })}
					{#if collection.timeUpdated} · {$_('collections_updated')} <Timestamp unix={collection.timeUpdated}/>{/if}
				</p>
			{/if}
		</div>
		{#if !creating}
			<div class="header-actions">
				<a class="button" href={workshopUrl(id)} target="_blank" rel="noreferrer"><ExternalLink size="1rem"/>{$_('steam_workshop')}</a>
				<button type="button" disabled={!!subscribing || !original.items.length} on:click={subscribeAll}>{#if subscribing}<Loading inline size="1rem"/>{:else}<BellPlus size="1rem"/>{/if}{$_('collections_subscribe_all')}</button>
				<button type="button" disabled={!original.items.length} on:click={downloadAll}><CloudDownload size="1rem"/>{$_('collections_download_all')}</button>
				<button type="button" class="icon-button" disabled={saving || reloading} on:click={reload} aria-label={$_('collections_reload')} title={$_('collections_reload')}><RefreshCw size="1rem"/></button>
				<button type="button" class="icon-button danger" disabled={saving} on:click={() => dispatch('delete', id)} aria-label={$_('collections_delete_one')} title={$_('collections_delete_one')}><Trash2 size="1rem"/></button>
			</div>
		{/if}
	</header>

	{#if subscribing}
		<div class="panel" role="status">
			<span>{$_('collections_subscribing', { values: { completed: subscribing.completed, total: subscribing.total } })}</span>
			<progress max={subscribing.total || 1} value={subscribing.completed}></progress>
		</div>
	{/if}
	{#if subscribeResult}
		<div class="panel result" role="status">
			<p>{$_('collections_subscribe_result', { values: { subscribed: subscribeResult.subscribed, already: subscribeResult.alreadySubscribed, skipped: subscribeResult.skipped } })}</p>
			{#if subscribeResult.error}<p class="error-text">{subscribeResult.error}</p>{/if}
			{#each subscribeResult.failures as failure}<p class="error-text">{failure.id}: {failure.error}</p>{/each}
			<button type="button" class="plain" on:click={() => subscribeResult = null}>{$_('close')}</button>
		</div>
	{/if}
	{#if subscribeError}<p class="message error" role="alert">{subscribeError}</p>{/if}
	{#if error}
		<div class="message error" role="alert">
			<p>{error}</p>
			{#if needsReload}<button type="button" disabled={reloading} on:click={reload}>{$_('collections_reload_review')}</button>{/if}
		</div>
	{/if}
	{#if saved}<p class="message success" role="status">{$_('collections_saved')}</p>{/if}

	{#if !creating}
		<div class="tabs" role="tablist">
			<button type="button" role="tab" aria-selected={tab === 'items'} class:active={tab === 'items'} on:click={() => tab = 'items'}>{$_('collections_tab_items', { values: { count: items.length } })}</button>
			<button type="button" role="tab" aria-selected={tab === 'details'} class:active={tab === 'details'} on:click={() => tab = 'details'}>{$_('collections_tab_details')}</button>
			<button type="button" role="tab" aria-selected={tab === 'problems'} class:active={tab === 'problems'} on:click={() => tab = 'problems'}>{$_('collections_tab_problems', { values: { count: problemCount } })}</button>
		</div>
	{/if}

	<div class="body">
		{#if tab === 'items'}
			<div class="list-toolbar">
				<input type="search" bind:value={itemQuery} placeholder={$_('collections_filter_items')} aria-label={$_('collections_filter_items')}/>
				<label class="inline">
					<span>{$_('workshop_sort')}</span>
					<select bind:value={itemSort}>
						{#each itemSortOptions as value}<option {value}>{$_('collections_items_sort_' + value)}</option>{/each}
					</select>
				</label>
				<span class="spacer"></span>
				<button type="button" class="plain" disabled={locked || !shownItems.length} on:click={() => selectedItems = new Set(shownItems)}>{$_('collections_select_shown')}</button>
				<button type="button" class="plain" disabled={!selectedItems.size} on:click={() => selectedItems = new Set()}>{$_('clear_selection')}</button>
				<button type="button" disabled={locked || !selectedItems.size} on:click={() => removeItems([...selectedItems])}><ListMinus size="1rem"/>{$_('collections_remove_selected', { values: { count: selectedItems.size } })}</button>
			</div>
			{#if entriesLoading}<p class="message status" role="status"><Loading inline size="1rem"/> {$_('collections_loading_items')}</p>{/if}
			{#if entriesError}<div class="message error" role="alert"><p>{entriesError}</p><button type="button" on:click={() => loadEntries(items)}>{$_('workshop_retry')}</button></div>{/if}
			{#if !items.length}
				<div class="empty">{$_('collections_no_items')}</div>
			{:else if !shownItems.length}
				<div class="empty">{$_('collections_no_item_matches')}</div>
			{/if}
			<ul class="items">
				{#each shownItems as item (item)}
					<CollectionItemRow id={item} entry={entries.get(item)} pending={!originalSet.has(item)} note={duplicates.has(item) ? $_('collections_also_in', { values: { names: otherNames(item) } }) : ''}>
						<input slot="start" type="checkbox" checked={selectedItems.has(item)} disabled={locked} on:change={event => toggleItem(item, event.currentTarget.checked)} aria-label={$_('collections_select_item', { values: { title: entries.get(item)?.title ?? item } })}/>
						<button type="button" class="icon-button" disabled={locked} on:click={() => removeItems([item])} aria-label={$_('collections_remove_item', { values: { title: entries.get(item)?.title ?? item } })}><X size="1rem"/></button>
					</CollectionItemRow>
				{/each}
			</ul>
			{#if removals.length}
				<div class="removed"><span>{$_('workshop_pending_removals')}</span>{#each removals as item (item)}<button type="button" disabled={locked} on:click={() => restoreItem(item)}>{entries.get(item)?.title ?? item} · {$_('workshop_undo')}</button>{/each}</div>
			{/if}

			<section class="add">
				<form on:submit|preventDefault={() => search()}>
					<label for="collection-search">{$_('collections_find_items')}</label>
					<div class="search-input"><input id="collection-search" type="search" bind:value={query} on:input={onSearchInput} disabled={locked} maxlength="512" placeholder={$_('workshop_search_placeholder')}/><button type="submit" disabled={locked || searching || !query.trim()}><Search size="1rem"/>{$_('workshop_search')}</button></div>
				</form>
				{#if searchError}<p class="message error" role="alert">{searchError}</p>{/if}
				{#if searching}<p class="message status" role="status"><Loading inline size="1rem"/> {$_('workshop_searching')}</p>{/if}
				{#if searched}
					<p class="hint" role="status">{results.length ? $_('workshop_search_results', { values: { count: total.toLocaleString(numberLocale) } }) : $_('workshop_search_empty')}</p>
					<ul class="items">
						{#each results as result (result.id)}
							{@const present = itemSet.has(result.id)}
							<CollectionItemRow id={result.id} entry={{ ...result, status: result.banned ? 'banned' : 'available' }}>
								<button type="button" disabled={locked || present || result.banned || adding.has(result.id)} on:click={() => addResult(result)}>{#if present}{$_('workshop_added')}{:else if adding.has(result.id)}<Loading inline size=".85rem"/>{:else}<Plus size=".85rem"/>{$_('workshop_add')}{/if}</button>
							</CollectionItemRow>
						{/each}
					</ul>
					{#if page * 50 < total}<button type="button" disabled={locked || searching} on:click={() => search(true)}>{$_('workshop_load_more')}</button>{/if}
				{/if}
				<form class="paste" on:submit|preventDefault={addPasted}>
					<label for="collection-paste">{$_('collections_paste_label')}</label>
					<textarea id="collection-paste" rows="3" bind:value={pasteText} disabled={locked || pasting} placeholder={$_('collections_paste_placeholder')}></textarea>
					<button type="submit" disabled={locked || pasting || !pasteText.trim()}>{#if pasting}<Loading inline size="1rem"/>{:else}<Plus size="1rem"/>{/if}{$_('collections_paste_add')}</button>
					{#if pasteMessage}<p class="hint" role="status">{pasteMessage}</p>{/if}
					{#if pasteError}<p class="message error" role="alert">{pasteError}</p>{/if}
				</form>
			</section>
		{:else if tab === 'details'}
			<div class="details">
				<div class="field">
					<label for="collection-title">{$_('collections_title')}</label>
					<input id="collection-title" type="text" bind:value={title} disabled={locked} on:input={() => { titleTouched = true; saved = false; }} aria-invalid={!!titleError && (titleTouched || !creating)}/>
					<p class="hint">{$_('workshop_description_size', { values: { bytes: titleBytes, max: titleMaxBytes } })}</p>
					{#if titleError && (titleTouched || !creating)}<p class="error-text">{titleError}</p>{/if}
				</div>
				<div class="field">
					<label for="collection-visibility">{$_('workshop_settings_visibility')}</label>
					<select id="collection-visibility" bind:value={visibility} disabled={locked} on:change={() => saved = false}>
						{#each visibilityOptions as option}<option value={option}>{$_('workshop_visibility_' + option)}</option>{/each}
					</select>
					<p class="hint">{$_('collections_visibility_help_' + visibility)}</p>
				</div>
				<div class="field">
					<span class="label">{$_('collections_image')}</span>
					<div class="image-row">
						{#if preview || previewUrl}<img src={preview?.dataUrl ?? previewUrl} alt=""/>{:else}<div class="image-placeholder large" aria-hidden="true">?</div>{/if}
						<div class="image-actions">
							<button type="button" disabled={locked} on:click={chooseImage}><ImagePlus size="1rem"/>{$_('collections_image_choose')}</button>
							{#if preview}<button type="button" class="plain" disabled={locked} on:click={() => preview = null}>{$_('workshop_undo')}</button>{/if}
							<p class="hint">{$_('collections_image_help')}</p>
						</div>
					</div>
				</div>
				<div class="description">
					<BBCodeEditor id="collection-description" label={$_('collections_description')} value={description} on:input={event => { description = event.detail; saved = false; }} disabled={locked} error={descriptionError} size={$_('workshop_description_size', { values: { bytes: descriptionBytes, max: descriptionMaxBytes } })} active={tab === 'details'} historyKey={`collection:${id}:${historyKey}`}/>
				</div>
			</div>
		{:else}
			<div class="problems">
				{#if entriesLoading}<p class="message status" role="status"><Loading inline size="1rem"/> {$_('collections_loading_items')}</p>{/if}
				{#if !problemCount && !duplicates.size}<div class="empty">{$_('collections_no_problems')}</div>{/if}
				{#each [['unavailable', unavailable], ['banned', banned], ['unsupported', unsupported]] as [kind, list] (kind)}
					{#if list.length}
						<section>
							<div class="group-heading"><h3>{$_('collections_problem_' + kind)} <span class="count">{list.length}</span></h3><button type="button" disabled={locked} on:click={() => removeItems(list)}><ListMinus size="1rem"/>{$_('collections_remove_all', { values: { count: list.length } })}</button></div>
							<p class="hint">{$_('collections_problem_' + kind + '_help')}</p>
							<ul class="items">
								{#each list as item (item)}
									<CollectionItemRow id={item} entry={entries.get(item)}>
										<button type="button" disabled={locked} on:click={() => removeItems([item])}>{$_('remove')}</button>
									</CollectionItemRow>
								{/each}
							</ul>
						</section>
					{/if}
				{/each}
				{#if missingRequirements.size}
					{@const addable = [...missingRequirements.keys()].map(item => entries.get(item)).filter(entry => entry?.status === 'available')}
					<section>
						<div class="group-heading"><h3>{$_('collections_problem_missing')} <span class="count">{missingRequirements.size}</span></h3><button type="button" disabled={locked || !addable.length} on:click={() => addItems(addable)}><Plus size="1rem"/>{$_('collections_add_all', { values: { count: addable.length } })}</button></div>
						<p class="hint">{$_('collections_problem_missing_help')}</p>
						<ul class="items">
							{#each [...missingRequirements] as [item, neededBy] (item)}
								{@const entry = entries.get(item)}
								<CollectionItemRow id={item} {entry} note={$_('collections_needed_by', { values: { names: neededBy.map(parent => entries.get(parent)?.title ?? parent).join(', ') } })}>
									<button type="button" disabled={locked || entry?.status !== 'available'} on:click={() => addItems([entry])}><Plus size=".85rem"/>{$_('workshop_add')}</button>
								</CollectionItemRow>
							{/each}
						</ul>
					</section>
				{/if}
				{#if duplicates.size}
					<section>
						<div class="group-heading"><h3>{$_('collections_problem_duplicates')} <span class="count">{duplicates.size}</span></h3></div>
						<p class="hint">{$_('collections_problem_duplicates_help')}</p>
						<ul class="items">
							{#each [...duplicates.keys()] as item (item)}
								<CollectionItemRow id={item} entry={entries.get(item)} note={$_('collections_also_in', { values: { names: otherNames(item) } })}>
									<button type="button" disabled={locked} on:click={() => removeItems([item])}>{$_('collections_remove_here')}</button>
									<button type="button" disabled={locked} on:click={() => dispatch('removeEverywhere', item)}>{$_('collections_remove_everywhere')}</button>
								</CollectionItemRow>
							{/each}
						</ul>
					</section>
				{/if}
			</div>
		{/if}
	</div>

	<footer>
		<div class="save-status" aria-live="polite">
			{#if creating}{$_('collections_create_help')}{:else if dirty}{$_('workshop_pending_summary', { values: { added: additions.length, removed: removals.length } })}{#if detailsChanged} · {$_('collections_details_changed')}{/if}{:else}{$_('collections_no_pending')}{/if}
		</div>
		<div class="save-actions">
			{#if creating}
				<button type="button" disabled={saving} on:click={back}>{$_('cancel')}</button>
			{:else}
				<button type="button" disabled={locked || !dirty} on:click={discard}>{$_('workshop_discard')}</button>
			{/if}
			<button type="button" class="primary" disabled={locked || !dirty || invalid} on:click={save}>{#if saving}<Loading inline size="1rem"/>{/if}{$_(saving ? 'workshop_saving' : creating ? 'collections_create' : 'collections_save')}</button>
		</div>
	</footer>
</section>

<style>
	.editor-view { display: flex; flex-direction: column; height: 100%; min-height: 0; }
	header { display: flex; align-items: center; flex-wrap: wrap; gap: .75rem 1rem; padding: 1rem 1.5rem .75rem; }
	header > img, header > .image-placeholder { width: 3.5rem; height: 3.5rem; border-radius: 4px; object-fit: cover; flex-shrink: 0; background: var(--control); }
	.image-placeholder { display: grid; place-items: center; color: var(--text-muted); background: var(--control); }
	.heading { flex: 1; min-width: 12rem; }
	h2 { margin: 0; font-size: 1.15rem; overflow-wrap: anywhere; }
	.heading p { margin: .3rem 0 0; color: var(--text-soft); font-size: .85rem; line-height: 1.6; }
	.header-actions { display: flex; flex-wrap: wrap; gap: .5rem; }
	button, select, input, textarea, .button { font: inherit; font-size: .9em; border: 1px solid var(--border-strong); border-radius: 4px; color: var(--text); background: var(--control); padding: .55rem .75rem; }
	button, .button { display: inline-flex; align-items: center; justify-content: center; gap: .5rem; cursor: pointer; flex-shrink: 0; text-decoration: none; line-height: 1.2; }
	button:hover:not(:disabled), .button:hover, select:hover:not(:disabled) { background: var(--control-hover); }
	button:disabled { opacity: .45; cursor: default; }
	button:focus-visible, select:focus-visible, input:focus-visible, textarea:focus-visible, .button:focus-visible, .tabs button:focus-visible { outline: 2px solid #127cff; outline-offset: 1px; }
	.icon-button { padding: .55rem; }
	.icon-button.danger:hover:not(:disabled) { background: var(--error); color: #fff; }
	button.plain { border-color: transparent; background: none; color: var(--link); }
	button.plain:hover:not(:disabled) { background: var(--control); }
	input, textarea, select { background: var(--fill); }
	input:disabled, textarea:disabled, select:disabled { color: var(--text-muted); }
	input[type='checkbox'] { accent-color: var(--neutral); flex-shrink: 0; }
	option { background: var(--control); color: var(--text); }
	.panel { display: flex; align-items: center; flex-wrap: wrap; gap: .5rem 1rem; margin: 0 1.5rem .5rem; padding: .5rem 1rem; border: 1px solid var(--border); border-radius: 4px; background: var(--bg-panel); color: var(--text-soft); }
	.panel.result { flex-direction: column; align-items: flex-start; }
	.panel p { margin: 0; line-height: 1.5; }
	progress { flex: 1; min-width: 8rem; height: .5rem; accent-color: var(--neutral); }
	.message { margin: 0 1.5rem .5rem; line-height: 1.5; overflow-wrap: anywhere; }
	.body .message { margin-inline: 0; }
	.message.error { padding: .75rem 1rem; border-radius: 4px; background: var(--error-dark); color: #fff; }
	.message.error p { margin: 0 0 .5rem; }
	.message.error p:last-child { margin: 0; }
	.message.success { padding: .75rem 1rem; border-radius: 4px; background: var(--success-dark); color: #fff; }
	.message.status { display: flex; align-items: center; gap: .5rem; color: var(--text-soft); }
	.error-text { color: var(--text-error-soft); margin: .35rem 0 0; font-size: .85rem; }
	.tabs { display: flex; gap: .25rem; padding: 0 1.5rem; border-bottom: 1px solid var(--border); }
	.tabs button { border: 0; border-bottom: 2px solid transparent; border-radius: 0; background: none; color: var(--text-muted); padding: .6rem .9rem; }
	.tabs button:hover { background: var(--bg-hover); color: var(--text); }
	.tabs button.active { border-bottom-color: var(--neutral); color: var(--text); }
	.body { flex: 1; min-height: 0; overflow: auto; padding: 1rem 1.5rem; }
	.list-toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: .5rem; margin-bottom: .75rem; }
	.list-toolbar input { min-width: 12rem; }
	.inline { display: flex; align-items: center; gap: .5rem; color: var(--text-soft); }
	.spacer { flex: 1; }
	.items { list-style: none; margin: 0 0 1rem; padding: 0; }
	.items :global(li > button) { padding: .45rem .7rem; }
	.empty { margin-bottom: 1rem; border-radius: 4px; padding: 1.25rem; background: var(--bg-preview); color: var(--text-muted); text-align: center; font-size: .9em; }
	.removed { display: flex; flex-wrap: wrap; align-items: center; gap: .5rem; margin-bottom: 1rem; color: var(--text-muted); font-size: .85em; }
	.add { border-top: 1px solid var(--border); padding-top: 1rem; }
	.add label, .field label, .label { display: block; font-weight: 600; font-size: .9em; margin-bottom: .5rem; }
	.search-input { display: flex; gap: .5rem; margin-bottom: .75rem; }
	.search-input input { flex: 1; min-width: 0; }
	.paste { margin-top: 1rem; padding-top: 1rem; border-top: 1px solid var(--border); }
	.paste textarea { display: block; width: 100%; resize: vertical; margin-bottom: .5rem; }
	.hint { margin: .35rem 0 .75rem; color: var(--text-muted); font-size: .85em; line-height: 1.5; }
	.details { display: flex; flex-direction: column; gap: 1rem; max-width: 70rem; }
	.field input, .field select { width: 100%; max-width: 40rem; }
	.field .hint { margin-bottom: 0; }
	.image-row { display: flex; align-items: flex-start; gap: 1rem; flex-wrap: wrap; }
	.image-row img, .image-placeholder.large { width: 7rem; height: 7rem; border-radius: 4px; object-fit: cover; }
	.image-actions { display: flex; flex-direction: column; align-items: flex-start; gap: .5rem; }
	.description { display: flex; min-height: 26rem; }
	.problems section { margin-bottom: 1.5rem; }
	.group-heading { display: flex; align-items: center; justify-content: space-between; gap: 1rem; flex-wrap: wrap; }
	h3 { margin: 0; font-size: .95em; }
	.count { color: var(--text-muted); background: var(--control); border-radius: 4px; padding: .1rem .4rem; font-size: .85em; margin-inline-start: .3rem; }
	footer { flex-shrink: 0; display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: .75rem; border-top: 1px solid var(--border); padding: .75rem 1.5rem; }
	.save-status { color: var(--text-muted); font-size: .85em; }
	.save-actions { display: flex; gap: .75rem; flex-wrap: wrap; margin-inline-start: auto; }
	.primary { background: var(--neutral); border-color: var(--neutral); color: #fff; }
	.primary:hover:not(:disabled) { background: var(--neutral-dark); }
</style>
