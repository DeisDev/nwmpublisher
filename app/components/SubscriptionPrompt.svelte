<script>
	import { _ } from 'svelte-i18n';
	import CirclePlus from '@lucide/svelte/icons/circle-plus';
	import CircleMinus from '@lucide/svelte/icons/circle-minus';
	import Modal from './Modal.svelte';
	import { subscriptionPrompt } from '../browse.js';

	$: prompt = $subscriptionPrompt;
	$: required = prompt?.kind === 'required';
	$: count = prompt?.items.length;
	$: title = prompt && $_(required ? 'browse_required_offer' : prompt.count > 1 ? 'browse_needed_by_many' : 'browse_needed_by_one', { values: { count } });
</script>

{#if prompt}
	<Modal active={true} cancel={() => prompt.answer(false)} padding="1.5rem">
		<div class="confirmation" role="dialog" aria-modal="true" aria-labelledby="subscription-prompt-title">
			<h2 id="subscription-prompt-title">{title}</h2>
			{#if !required}<p class="warning">{$_('browse_needed_by_note')}</p>{/if}
			<ul class="names">
				{#each prompt.items as item (item.id)}<li>{item.title ?? item.id}</li>{/each}
			</ul>
			<div class="actions">
				<button type="button" on:click={() => prompt.answer(false)}>{$_(required ? 'browse_required_skip' : 'cancel')}</button>
				{#if required}
					<button type="button" class="subscribe" on:click={() => prompt.answer(true)}><CirclePlus size="1rem"/>{$_('browse_subscribe_count', { values: { count } })}</button>
				{:else}
					<button type="button" class="unsubscribe" on:click={() => prompt.answer(true)}><CircleMinus size="1rem"/>{$_('unsubscribe')}</button>
				{/if}
			</div>
		</div>
	</Modal>
{/if}

<style>
	.confirmation { width: min(28rem, 70vw); }
	h2 { margin: 0 0 1rem; font-size: 1.1rem; overflow-wrap: anywhere; }
	.warning { margin: 0 0 1rem; padding-inline-start: .75rem; border-inline-start: 3px solid var(--error); line-height: 1.5; }
	.names { max-height: 10rem; overflow-y: auto; margin: 0; padding: .5rem .75rem; padding-inline-start: 1.75rem; border: 1px solid var(--border); border-radius: 4px; font-size: .85rem; line-height: 1.5; overflow-wrap: anywhere; }
	.actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: .75rem; margin-top: 1.5rem; }
	button { display: inline-flex; align-items: center; gap: .5rem; padding: .65rem .85rem; border: 0; border-radius: .25rem; color: var(--text); background: var(--control); font: inherit; cursor: pointer; }
	button.subscribe { background: var(--neutral); color: #fff; }
	button.unsubscribe { background: var(--error); color: #fff; }
	button:hover { filter: brightness(1.15); }
	button:focus-visible { outline: 2px solid var(--text); outline-offset: 2px; }
</style>
