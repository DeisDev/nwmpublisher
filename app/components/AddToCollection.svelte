<script>
	import { _ } from 'svelte-i18n';
	import { Channel, invoke } from '@tauri-apps/api/core';
	import Check from '@lucide/svelte/icons/check';
	import Modal from './Modal.svelte';
	import Loading from './Loading.svelte';
	import { playSound } from '../sounds';
	import { taskMessage } from '../transactions.js';
	import { collectionPicker, collections, compareTitles, ensureCollections, loadCollections } from '../collections.js';

	let wanted = new Map();
	let busy = false;
	let progress = null;
	let error = null;
	let failures = [];

	$: request = $collectionPicker;
	$: reset(request);
	$: items = request?.items ?? [];
	$: choices = [...$collections.items].sort((a, b) => compareTitles(a.fields.title, b.fields.title));
	$: changes = choices.map(collection => change(collection, wanted)).filter(Boolean);

	function reset(request) {
		wanted = new Map();
		error = null;
		failures = [];
		progress = null;
		if (request) ensureCollections();
	}

	function present(collection) {
		return items.filter(id => collection.fields.items.includes(id));
	}

	function initial(collection) {
		const count = present(collection).length;
		return count === items.length ? 'all' : count ? 'some' : 'none';
	}

	const stateOf = (collection, wanted) => wanted.get(collection.id) ?? initial(collection);

	function cycle(collection) {
		const start = initial(collection);
		const order = start === 'some' ? ['some', 'all', 'none'] : start === 'all' ? ['all', 'none'] : ['none', 'all'];
		const next = order[(order.indexOf(stateOf(collection, wanted)) + 1) % order.length];
		const updated = new Map(wanted);
		if (next === start) updated.delete(collection.id);
		else updated.set(collection.id, next);
		wanted = updated;
	}

	function change(collection, wanted) {
		const state = wanted.get(collection.id);
		if (!state) return null;
		const members = present(collection);
		if (state === 'all') return { collectionId: collection.id, add: items.filter(id => !members.includes(id)), remove: [] };
		if (state === 'none') return { collectionId: collection.id, add: [], remove: members };
		return null;
	}

	function cancel() {
		if (!busy) $collectionPicker = null;
	}

	async function apply() {
		const sent = changes;
		const total = sent.reduce((count, item) => count + item.add.length + item.remove.length, 0);
		busy = true;
		error = null;
		failures = [];
		progress = { completed: 0, total };
		try {
			const { outcomes } = await invoke('apply_collection_changes', { changes: sent, onProgress: new Channel(update => progress = update) });
			const titles = new Map($collections.items.map(collection => [collection.id, collection.fields.title]));
			failures = outcomes.filter(outcome => outcome.error).map(outcome => ({ title: titles.get(outcome.id) ?? outcome.id, error: outcome.error }));
			const removing = new Map(sent.map(item => [item.collectionId, new Set(item.remove)]));
			let added = 0;
			let removed = 0;
			for (const outcome of outcomes) {
				for (const id of outcome.changed) removing.get(outcome.id)?.has(id) ? removed++ : added++;
			}
			taskMessage($_('collections_items_changed', { values: { added, removed } }));
			if (added || removed) playSound('success');
			if (!failures.length) $collectionPicker = null;
		} catch (err) {
			error = String(err);
		} finally {
			busy = false;
			progress = null;
			wanted = new Map();
			loadCollections();
		}
	}
</script>

{#if request}
	<Modal active={true} {cancel} padding="1.5rem">
		<div class="picker" role="dialog" aria-modal="true" aria-labelledby="collection-picker-title">
			<h2 id="collection-picker-title">{$_('collections_picker_title', { values: { count: items.length } })}</h2>
			{#if !$collections.loaded && !$collections.error}
				<p class="status" role="status"><Loading inline size="1rem"/> {$_('collections_loading')}</p>
			{:else if $collections.error && !$collections.loaded}
				<div class="error" role="alert"><p>{$_('collections_load_error', { values: { error: $collections.error } })}</p><button type="button" on:click={loadCollections}>{$_('workshop_retry')}</button></div>
			{:else if !choices.length}
				<p class="status">{$_('collections_add_none')}</p>
			{:else}
				<p class="help">{$_('collections_picker_help')}</p>
				<ul>
					{#each choices as collection (collection.id)}
						{@const state = stateOf(collection, wanted)}
						{@const count = present(collection).length}
						<li>
							<button type="button" role="checkbox" aria-checked={state === 'all' ? 'true' : state === 'some' ? 'mixed' : 'false'} class:changed={wanted.has(collection.id)} disabled={busy} on:click={() => cycle(collection)}>
								<span class="box {state}" aria-hidden="true">{#if state === 'all'}<Check size=".8rem" strokeWidth={3}/>{:else if state === 'some'}<span class="dash"></span>{/if}</span>
								<span class="text">
									<span class="title">{collection.fields.title}</span>
									<small>{$_('collections_item_count', { values: { count: collection.fields.items.length } })} · {$_('workshop_visibility_' + collection.fields.visibility)}{#if count && items.length > 1}{' · '}{$_('collections_some_added', { values: { count } })}{/if}{#if wanted.has(collection.id)}{' · '}<span class="pending">{$_(state === 'all' ? 'collections_will_add' : 'collections_will_remove')}</span>{/if}</small>
								</span>
							</button>
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
				<button type="button" class="primary" disabled={busy || !changes.length} on:click={apply}>{#if busy}<Loading inline size="1rem"/>{:else}<Check size="1rem"/>{/if}{$_('collections_apply')}</button>
			</div>
		</div>
	</Modal>
{/if}

<style>
	.picker { width: min(30rem, 70vw); }
	h2 { margin: 0 0 1rem; font-size: 1.1rem; }
	p { margin: 0 0 1rem; line-height: 1.5; }
	.help { color: var(--text-muted); font-size: .85rem; }
	.status { display: flex; align-items: center; gap: .5rem; color: var(--text-soft); }
	ul { list-style: none; margin: 0 0 1rem; padding: 0; max-height: min(22rem, 50vh); overflow-y: auto; border: 1px solid var(--border); border-radius: 4px; }
	li + li { border-top: 1px solid var(--border); }
	li > button { display: flex; align-items: center; gap: .75rem; width: 100%; padding: .6rem .75rem; border: 0; border-radius: 0; background: none; color: var(--text); font: inherit; text-align: start; cursor: pointer; }
	li > button:hover:not(:disabled) { background: var(--bg-hover); filter: none; }
	li > button.changed { box-shadow: inset 3px 0 0 var(--neutral); }
	.box { display: flex; align-items: center; justify-content: center; flex-shrink: 0; width: 1rem; height: 1rem; border: 2px solid var(--border-strong); border-radius: 3px; color: #fff; }
	.box.all, .box.some { border-color: var(--neutral); background: var(--neutral); }
	.dash { width: .5rem; height: 2px; background: #fff; }
	.text { display: flex; flex-direction: column; min-width: 0; }
	.title { overflow-wrap: anywhere; }
	small { color: var(--text-muted); font-size: .8rem; margin-top: .15rem; }
	.pending { color: var(--text); }
	progress { width: 100%; height: .5rem; accent-color: var(--neutral); margin-bottom: 1rem; }
	.error { color: var(--text-error-soft); overflow-wrap: anywhere; }
	.error p { margin-bottom: .5rem; }
	.failures { padding: .5rem .75rem; max-height: 10rem; color: var(--text-error-soft); font-size: .85rem; line-height: 1.5; overflow-wrap: anywhere; }
	.failures li + li { border: 0; }
	.actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: .75rem; }
	.actions button { display: inline-flex; align-items: center; gap: .5rem; padding: .65rem .85rem; border: 0; border-radius: .25rem; color: var(--text); background: var(--control); font: inherit; cursor: pointer; }
	.actions button.primary { background: var(--neutral); color: #fff; }
	.actions button:hover:not(:disabled) { filter: brightness(1.15); }
	button:disabled { opacity: .45; cursor: default; }
	button:focus-visible { outline: 2px solid var(--text); outline-offset: 2px; }
</style>
