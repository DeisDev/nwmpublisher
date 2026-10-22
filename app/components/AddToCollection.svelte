<script>
	import { _ } from 'svelte-i18n';
	import { Channel, invoke } from '@tauri-apps/api/core';
	import ListPlus from '@lucide/svelte/icons/list-plus';
	import Modal from './Modal.svelte';
	import Loading from './Loading.svelte';
	import { taskMessage } from '../transactions.js';
	import { collectionPicker, collections, compareTitles, ensureCollections, loadCollections } from '../collections.js';

	let chosen = new Set();
	let busy = false;
	let progress = null;
	let error = null;
	let failures = [];

	$: request = $collectionPicker;
	$: reset(request);
	$: items = request?.items ?? [];
	$: choices = [...$collections.items].sort((a, b) => compareTitles(a.fields.title, b.fields.title));

	function reset(request) {
		chosen = new Set();
		error = null;
		failures = [];
		progress = null;
		if (request) ensureCollections();
	}

	function present(collection) {
		return items.filter(id => collection.fields.items.includes(id)).length;
	}

	function toggle(id, checked) {
		const next = new Set(chosen);
		if (checked) next.add(id);
		else next.delete(id);
		chosen = next;
	}

	function cancel() {
		if (!busy) $collectionPicker = null;
	}

	async function add() {
		const targets = [...chosen];
		busy = true;
		error = null;
		failures = [];
		progress = { completed: 0, total: targets.length * items.length };
		try {
			const outcomes = await invoke('change_collection_items', { collections: targets, items, add: true, onProgress: new Channel(update => progress = update) });
			const titles = new Map($collections.items.map(collection => [collection.id, collection.fields.title]));
			failures = outcomes.filter(outcome => outcome.error).map(outcome => ({ title: titles.get(outcome.id) ?? outcome.id, error: outcome.error }));
			const added = outcomes.reduce((count, outcome) => count + outcome.changed.length, 0);
			taskMessage($_('collections_items_added', { values: { count: added } }));
			if (!failures.length) $collectionPicker = null;
		} catch (err) {
			error = String(err);
		} finally {
			busy = false;
			progress = null;
			chosen = new Set();
			loadCollections();
		}
	}
</script>

{#if request}
	<Modal active={true} {cancel} padding="1.5rem">
		<div class="picker" role="dialog" aria-modal="true" aria-labelledby="add-to-collection-title">
			<h2 id="add-to-collection-title">{$_('collections_add_title', { values: { count: items.length } })}</h2>
			{#if !$collections.loaded && !$collections.error}
				<p class="status" role="status"><Loading inline size="1rem"/> {$_('collections_loading')}</p>
			{:else if $collections.error && !$collections.loaded}
				<div class="error" role="alert"><p>{$_('collections_load_error', { values: { error: $collections.error } })}</p><button type="button" on:click={loadCollections}>{$_('workshop_retry')}</button></div>
			{:else if !choices.length}
				<p class="status">{$_('collections_add_none')}</p>
			{:else}
				<ul>
					{#each choices as collection (collection.id)}
						{@const count = present(collection)}
						{@const complete = count === items.length}
						<li>
							<label class:complete>
								<input type="checkbox" disabled={busy || complete} checked={complete || chosen.has(collection.id)} on:change={event => toggle(collection.id, event.currentTarget.checked)}/>
								<span class="text">
									<span class="title">{collection.fields.title}</span>
									<small>{$_('collections_item_count', { values: { count: collection.fields.items.length } })} · {$_('workshop_visibility_' + collection.fields.visibility)}{#if complete}{' · '}{$_('collections_already_added')}{:else if count}{' · '}{$_('collections_some_added', { values: { count } })}{/if}</small>
								</span>
							</label>
						</li>
					{/each}
				</ul>
			{/if}
			{#if progress}
				<progress max={progress.total} value={progress.completed} aria-label={$_('collections_saving')}></progress>
			{/if}
			{#if error}<p class="error" role="alert">{error}</p>{/if}
			{#if failures.length}
				<ul class="failures" role="alert">
					{#each failures as failure}<li><strong>{failure.title}</strong>: {failure.error}</li>{/each}
				</ul>
			{/if}
			<div class="actions">
				<button type="button" disabled={busy} on:click={cancel}>{$_(failures.length ? 'close' : 'cancel')}</button>
				<button type="button" class="primary" disabled={busy || !chosen.size} on:click={add}>{#if busy}<Loading inline size="1rem"/>{:else}<ListPlus size="1rem"/>{/if}{$_('collections_add_button', { values: { count: chosen.size } })}</button>
			</div>
		</div>
	</Modal>
{/if}

<style>
	.picker { width: min(30rem, 70vw); }
	h2 { margin: 0 0 1rem; font-size: 1.1rem; }
	p { margin: 0 0 1rem; line-height: 1.5; }
	.status { display: flex; align-items: center; gap: .5rem; color: var(--text-soft); }
	ul { list-style: none; margin: 0 0 1rem; padding: 0; max-height: min(22rem, 50vh); overflow-y: auto; border: 1px solid var(--border); border-radius: 4px; }
	li + li { border-top: 1px solid var(--border); }
	label { display: flex; align-items: center; gap: .75rem; padding: .6rem .75rem; cursor: pointer; }
	label:hover { background: var(--bg-hover); }
	label.complete { cursor: default; color: var(--text-muted); }
	.text { display: flex; flex-direction: column; min-width: 0; }
	.title { overflow-wrap: anywhere; }
	small { color: var(--text-muted); font-size: .8rem; margin-top: .15rem; }
	input { accent-color: var(--neutral); flex-shrink: 0; }
	progress { width: 100%; height: .5rem; accent-color: var(--neutral); margin-bottom: 1rem; }
	.error { color: var(--text-error-soft); overflow-wrap: anywhere; }
	.error p { margin-bottom: .5rem; }
	.failures { padding: .5rem .75rem; max-height: 10rem; color: var(--text-error-soft); font-size: .85rem; line-height: 1.5; overflow-wrap: anywhere; }
	.failures li + li { border: 0; }
	.actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: .75rem; }
	button { display: inline-flex; align-items: center; gap: .5rem; padding: .65rem .85rem; border: 0; border-radius: .25rem; color: var(--text); background: var(--control); font: inherit; cursor: pointer; }
	button.primary { background: var(--neutral); color: #fff; }
	button:hover:not(:disabled) { filter: brightness(1.15); }
	button:disabled { opacity: .45; cursor: default; }
	button:focus-visible, input:focus-visible { outline: 2px solid var(--text); outline-offset: 2px; }
</style>
