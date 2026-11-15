<script>
	import { _ } from 'svelte-i18n';
	import { Channel, invoke } from '@tauri-apps/api/core';
	import CirclePlus from '@lucide/svelte/icons/circle-plus';
	import CloudDownload from '@lucide/svelte/icons/cloud-download';
	import Copy from '@lucide/svelte/icons/copy';
	import GitCompare from '@lucide/svelte/icons/git-compare';
	import Loading from './Loading.svelte';
	import CollectionItemRow from './CollectionItemRow.svelte';
	import CollectionCompare from './CollectionCompare.svelte';
	import { taskError, taskMessage } from '../transactions.js';
	import { playSound } from '../sounds.js';
	import { collections, compareTitles, ensureCollections, loadCollections } from '../collections.js';
	import { browseJob, downloadItems, subscribeCollection } from '../browse.js';

	export let workshop;
	export let children;
	export let onOpen;

	let copying = false;
	let compareWith = '';
	let comparing = null;

	ensureCollections();

	$: items = children.map(child => child.workshop);
	$: ids = items.map(item => String(item.id));
	$: addonIds = items.filter(item => !item.dead && item.kind === 'addon').map(item => String(item.id));
	$: entries = new Map(items.map(item => [String(item.id), {
		id: String(item.id),
		title: item.dead ? null : item.title,
		previewUrl: item.previewUrl,
		fileSize: item.fileSize,
		status: item.dead ? 'unavailable' : item.banned ? 'banned' : 'available',
	}]));
	$: mine = [...$collections.items].sort((a, b) => compareTitles(a.fields.title, b.fields.title));

	async function copy() {
		copying = true;
		try {
			const created = await invoke('create_collection', { draft: { title: workshop.title, description: '', visibility: 'private', preview: null } });
			if (created.error) taskError($_('browse_copy_details_failed', { values: { error: created.error } }));
			const { outcomes } = await invoke('apply_collection_changes', { changes: [{ collectionId: created.id, add: ids }], onProgress: new Channel(() => {}) });
			const failure = outcomes.find(outcome => outcome.error);
			if (failure) {
				playSound('error');
				taskError($_('browse_copy_partial', { values: { error: failure.error } }));
			} else {
				playSound('success');
				taskMessage($_('browse_copied', { values: { title: workshop.title } }));
			}
		} catch (error) {
			playSound('error');
			taskError($_('browse_copy_failed', { values: { error: String(error) } }));
		} finally {
			copying = false;
			loadCollections();
		}
	}

	function compare() {
		const right = $collections.items.find(item => item.id === compareWith);
		if (right) comparing = { left: { fields: { title: workshop.title, items: ids } }, right };
	}
</script>

{#if comparing}
	<div class="compare">
		<CollectionCompare left={comparing.left} right={comparing.right} on:close={() => comparing = null}/>
	</div>
{:else}
	<section>
		<div class="actions">
			<button type="button" class="primary" disabled={!!$browseJob || !ids.length} on:click={() => subscribeCollection(workshop.id, ids.length)}><CirclePlus size="1rem"/>{$_('collections_subscribe_all')}</button>
			<button type="button" disabled={!addonIds.length} on:click={() => downloadItems(addonIds)}><CloudDownload size="1rem"/>{$_('collections_download_all')}</button>
			<button type="button" disabled={copying || !ids.length} on:click={copy}>{#if copying}<Loading inline size="1rem"/>{:else}<Copy size="1rem"/>{/if}{$_('browse_copy_collection')}</button>
			<span class="compare-with">
				<select bind:value={compareWith} aria-label={$_('browse_compare_with')} disabled={!mine.length}>
					<option value="">{$_(mine.length ? 'browse_compare_with' : 'collections_add_none_short')}</option>
					{#each mine as item (item.id)}<option value={item.id}>{item.fields.title}</option>{/each}
				</select>
				<button type="button" disabled={!compareWith} on:click={compare}><GitCompare size="1rem"/>{$_('collections_compare')}</button>
			</span>
		</div>
		<h3>{$_('browse_items')} <span class="count">{ids.length}</span></h3>
		<ul>
			{#each ids as id (id)}
				{@const entry = entries.get(id)}
				<CollectionItemRow {id} {entry}>
					{#if entry.status !== 'unavailable'}<button type="button" on:click={() => onOpen(id)}>{$_('open')}</button>{/if}
				</CollectionItemRow>
			{/each}
		</ul>
	</section>
{/if}

<style>
	section, .compare {
		margin-top: 1.25rem;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: .5rem;
		margin-bottom: 1rem;
	}
	.compare-with {
		display: inline-flex;
		gap: .5rem;
		margin-inline-start: auto;
	}
	h3 {
		margin: 0 0 .6rem;
		font-size: 1rem;
	}
	.count {
		color: var(--text-muted);
		font-weight: normal;
	}
	ul {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	button, select {
		display: inline-flex;
		align-items: center;
		gap: .5rem;
		padding: .5rem .75rem;
		border: 1px solid var(--border-strong);
		border-radius: 4px;
		background: var(--control);
		color: var(--text);
		font: inherit;
		cursor: pointer;
	}
	select {
		max-width: 16rem;
	}
	button:hover:not(:disabled), select:hover:not(:disabled) {
		background: var(--control-hover);
	}
	button:disabled, select:disabled {
		opacity: .45;
		cursor: default;
	}
	button.primary {
		border-color: var(--neutral);
		background: var(--neutral);
		color: #fff;
	}
	button:focus-visible, select:focus-visible {
		outline: 2px solid var(--text);
		outline-offset: 2px;
	}
</style>
