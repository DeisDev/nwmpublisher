<script>
	import { _ } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import { createEventDispatcher, onDestroy } from 'svelte';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Loading from './Loading.svelte';
	import CollectionItemRow from './CollectionItemRow.svelte';

	export let left;
	export let right;

	const dispatch = createEventDispatcher();
	let entries = new Map();
	let loading = true;
	let error = null;
	let destroyed = false;
	onDestroy(() => destroyed = true);

	$: leftItems = new Set(left.fields.items);
	$: rightItems = new Set(right.fields.items);
	$: groups = [
		{ key: 'left', title: $_('collections_only_in', { values: { name: left.fields.title } }), items: left.fields.items.filter(item => !rightItems.has(item)) },
		{ key: 'both', title: $_('collections_in_both'), items: left.fields.items.filter(item => rightItems.has(item)) },
		{ key: 'right', title: $_('collections_only_in', { values: { name: right.fields.title } }), items: right.fields.items.filter(item => !leftItems.has(item)) },
	];

	async function load() {
		loading = true;
		error = null;
		try {
			const lookup = await invoke('collection_entries', { references: [...new Set([...left.fields.items, ...right.fields.items])] });
			if (!destroyed) entries = new Map(lookup.entries.map(entry => [entry.id, entry]));
		} catch (err) {
			if (!destroyed) error = String(err);
		} finally {
			if (!destroyed) loading = false;
		}
	}
	load();
</script>

<section class="compare">
	<header>
		<button type="button" class="icon-button" on:click={() => dispatch('close')} aria-label={$_('collections_back')} title={$_('collections_back')}><ArrowLeft size="1rem"/></button>
		<h2>{$_('collections_compare_title', { values: { left: left.fields.title, right: right.fields.title } })}</h2>
	</header>
	{#if loading}<p class="message" role="status"><Loading inline size="1rem"/> {$_('collections_loading_items')}</p>{/if}
	{#if error}<div class="message error" role="alert"><p>{error}</p><button type="button" on:click={load}>{$_('workshop_retry')}</button></div>{/if}
	<div class="columns">
		{#each groups as group (group.key)}
			<section>
				<h3>{group.title} <span class="count">{group.items.length}</span></h3>
				{#if !group.items.length}<div class="empty">{$_('collections_compare_empty')}</div>{/if}
				<ul>
					{#each group.items as item (item)}
						<CollectionItemRow id={item} entry={entries.get(item)}/>
					{/each}
				</ul>
			</section>
		{/each}
	</div>
</section>

<style>
	.compare { display: flex; flex-direction: column; height: 100%; min-height: 0; }
	header { display: flex; align-items: center; gap: 1rem; padding: 1rem 1.5rem .75rem; }
	h2 { margin: 0; font-size: 1.15rem; overflow-wrap: anywhere; }
	button { display: inline-flex; align-items: center; gap: .5rem; font: inherit; font-size: .9em; border: 1px solid var(--border-strong); border-radius: 4px; color: var(--text); background: var(--control); padding: .55rem .75rem; cursor: pointer; }
	button:hover { background: var(--control-hover); }
	button:focus-visible { outline: 2px solid #127cff; outline-offset: 1px; }
	.icon-button { padding: .55rem; }
	.message { display: flex; align-items: center; gap: .5rem; margin: 0 1.5rem .5rem; color: var(--text-soft); }
	.message.error { display: block; padding: .75rem 1rem; border-radius: 4px; background: var(--error-dark); color: #fff; overflow-wrap: anywhere; }
	.message.error p { margin: 0 0 .5rem; }
	.columns { flex: 1; min-height: 0; overflow: auto; display: grid; grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr)); gap: 1rem; padding: .5rem 1.5rem 1.5rem; align-items: start; }
	h3 { margin: 0 0 .75rem; font-size: .95em; overflow-wrap: anywhere; }
	.count { color: var(--text-muted); background: var(--control); border-radius: 4px; padding: .1rem .4rem; font-size: .85em; margin-inline-start: .3rem; }
	ul { list-style: none; margin: 0; padding: 0; }
	.empty { border-radius: 4px; padding: 1rem; background: var(--bg-preview); color: var(--text-muted); text-align: center; font-size: .85em; }
</style>
