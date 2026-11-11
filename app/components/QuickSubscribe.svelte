<script>
	import { _ } from 'svelte-i18n';
	import Plus from '@lucide/svelte/icons/plus';
	import Check from '@lucide/svelte/icons/check';
	import Loading from './Loading.svelte';
	import { browseJob, quickBusy, subscribedIds, toggleSubscription } from '../browse.js';

	export let workshop;
	export let overlay = false;

	$: id = String(workshop.id);
	$: subscribed = $subscribedIds.has(id);
	$: busy = $quickBusy === id;
	$: unavailable = !!$quickBusy || !!$browseJob;
	$: label = $_(subscribed ? 'unsubscribe' : 'browse_subscribe');
</script>

<!-- aria-disabled keeps clicks on a waiting button from reaching the card underneath. -->
<button type="button" class="quick-subscribe" class:overlay class:subscribed class:busy tabindex="-1" aria-disabled={unavailable} title={label} aria-label={label} on:mousedown|preventDefault on:click|stopPropagation={() => toggleSubscription(workshop)}>
	{#if busy}<Loading inline size="1rem"/>{:else if subscribed}<Check size="1rem" strokeWidth={3}/>{:else}<Plus size="1rem" strokeWidth={3}/>{/if}
</button>

<style>
	.quick-subscribe {
		display: flex;
		align-items: center;
		justify-content: center;
		flex-shrink: 0;
		width: 1.75rem;
		height: 1.75rem;
		padding: 0;
		border: 0;
		border-radius: 4px;
		background: var(--neutral);
		color: #fff;
		cursor: pointer;
		opacity: 0;
		transition: opacity .1s, filter .1s;
	}
	.quick-subscribe.overlay {
		position: absolute;
		bottom: .4rem;
		inset-inline-end: .4rem;
		z-index: 1;
		box-shadow: 0 0 3px rgb(0 0 0 / 60%);
	}
	.quick-subscribe.subscribed {
		background: var(--success);
	}
	:global(.addon:hover) .quick-subscribe, :global(.addon-row:hover) .quick-subscribe, :global(:focus-visible) > :global(.addon) .quick-subscribe, :global(:focus-visible) > :global(.addon-row) .quick-subscribe, .quick-subscribe.busy {
		opacity: 1;
	}
	.quick-subscribe:hover:not([aria-disabled='true']) {
		filter: brightness(1.15);
	}
	.quick-subscribe[aria-disabled='true'] {
		cursor: default;
	}
	.quick-subscribe[aria-disabled='true']:not(.busy) {
		filter: grayscale(.6);
	}
</style>
