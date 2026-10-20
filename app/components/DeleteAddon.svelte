<script>
	import { _ } from 'svelte-i18n';
	import { get } from 'svelte/store';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import CircleMinus from '@lucide/svelte/icons/circle-minus';
	import Modal from './Modal.svelte';
	import { deleteAddon, deleteRequest, removeAddons, unsubscribeAddon } from '../addon-removal.js';
	import { saveSettings, settings } from '../settings.js';

	let permanent = false;
	let remember = false;
	let busy = false;

	function reset(request) {
		permanent = request?.items.length > 1 && get(settings).addon_delete_mode === 'permanent';
		remember = false;
	}
	$: reset($deleteRequest);

	$: request = $deleteRequest;
	$: bulk = request?.items.length > 1;
	$: title = request && $_(
		request.unsubscribe
			? (request.deleteFiles ? (bulk ? 'bulk_unsubscribe_delete_title' : 'addon_unsubscribe_delete_title') : (bulk ? 'bulk_unsubscribe_title' : 'addon_unsubscribe_title'))
			: (bulk ? 'bulk_delete_title' : 'addon_delete_title'),
		{ values: bulk ? { count: request.items.length } : { name: request.items[0].name } },
	);

	function cancel() {
		if (!busy) $deleteRequest = null;
	}

	async function confirm() {
		const { items, unsubscribe, deleteFiles, onDone } = $deleteRequest;
		if (bulk) {
			$deleteRequest = null;
			const outcomes = await removeAddons(items, unsubscribe, deleteFiles, permanent);
			if (outcomes) onDone?.(outcomes);
			return;
		}
		const [{ path, name }] = items;
		busy = true;
		if (remember) saveSettings({ addon_delete_mode: permanent ? 'permanent' : 'trash' }).catch(() => {});
		const result = unsubscribe ? await unsubscribeAddon(path, name, deleteFiles, permanent) : await deleteAddon(path, name, permanent);
		busy = false;
		$deleteRequest = null;
		if (result) onDone?.(result);
	}
</script>

{#if request}
	<Modal active={true} {cancel} padding="1.5rem">
		<div class="confirmation" role="dialog" aria-modal="true" aria-labelledby="delete-addon-title">
			<h2 id="delete-addon-title">{title}</h2>
			{#if request.unsubscribe && request.deleteFiles}<p>{$_('addon_unsubscribe_delete_note')}</p>{/if}
			{#if bulk}
				<ul class="names">
					{#each request.items as item (item.path)}<li title={item.path}>{item.name}</li>{/each}
				</ul>
			{:else}
				<p class="path">{request.items[0].path}</p>
			{/if}
			{#if request.deleteFiles}
				<fieldset disabled={busy}>
					<legend>{$_('cleaner_removal')}</legend>
					<label><input type="radio" name="delete-addon-mode" value={false} bind:group={permanent}>{$_('cleaner_trash')}</label>
					<label><input type="radio" name="delete-addon-mode" value={true} bind:group={permanent}>{$_('cleaner_permanent')}</label>
				</fieldset>
				{#if permanent}<p class="warning">{$_('addon_delete_permanent_warning')}</p>{/if}
				{#if !bulk}
					<label class="remember"><input type="checkbox" bind:checked={remember} disabled={busy}>{$_('addon_delete_remember')}</label>
					{#if remember}<small>{$_('addon_delete_remember_help')}</small>{/if}
				{/if}
			{/if}
			<div class="actions">
				<button type="button" disabled={busy} on:click={cancel}>{$_('cancel')}</button>
				{#if request.deleteFiles}
					<button type="button" class="remove" disabled={busy} on:click={confirm}><Trash2 size="1rem"/>{$_(permanent ? 'cleaner_permanent' : 'cleaner_trash')}</button>
				{:else}
					<button type="button" class="unsubscribe" disabled={busy} on:click={confirm}><CircleMinus size="1rem"/>{$_('addon_unsubscribe')}</button>
				{/if}
			</div>
		</div>
	</Modal>
{/if}

<style>
	.confirmation { width: min(28rem, 70vw); }
	h2 { margin: 0 0 .5rem; font-size: 1.1rem; overflow-wrap: anywhere; }
	p { line-height: 1.5; margin: 0 0 1rem; }
	.path, small { color: var(--text-muted); font-size: .8rem; overflow-wrap: anywhere; }
	small { display: block; margin-top: .4rem; }
	.names { max-height: 10rem; overflow-y: auto; margin: 0 0 1rem; padding: .5rem .75rem; padding-inline-start: 1.75rem; border: 1px solid var(--border); border-radius: 4px; font-size: .85rem; line-height: 1.5; overflow-wrap: anywhere; }
	fieldset { display: flex; flex-direction: column; gap: .5rem; margin: 0 0 1rem; padding: .75rem; border: 1px solid var(--border); border-radius: 4px; }
	legend { padding: 0 .35rem; font-size: .85rem; color: var(--text-muted); }
	label { display: flex; align-items: center; gap: .5rem; cursor: pointer; }
	input { accent-color: var(--neutral); }
	.warning { color: var(--text); border-inline-start: 3px solid var(--error); padding-inline-start: .75rem; }
	.actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: .75rem; margin-top: 1.5rem; }
	button { display: inline-flex; align-items: center; gap: .5rem; padding: .65rem .85rem; border: 0; border-radius: .25rem; color: var(--text); background: var(--control); font: inherit; cursor: pointer; }
	button.remove { background: var(--error); color: #fff; }
	button.unsubscribe { background: var(--neutral); color: #fff; }
	button:hover:not(:disabled) { filter: brightness(1.15); }
	button:disabled { opacity: .45; cursor: default; }
	button:focus-visible, input:focus-visible { outline: 2px solid var(--text); outline-offset: 2px; }
</style>
