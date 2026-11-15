<script>
	import { _ } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import { onDestroy } from 'svelte';
	import Eye from '@lucide/svelte/icons/eye';
	import Modal from './Modal.svelte';
	import Loading from './Loading.svelte';
	import BBCode from './BBCode.svelte';
	import BrowseGallery from './BrowseGallery.svelte';

	export let workshop;
	export let onOpen;
	export let onClose;

	let details = null;
	let error = null;
	let destroyed = false;
	onDestroy(() => destroyed = true);

	invoke('browse_item', { reference: String(workshop.id) }).then(result => {
		if (!destroyed) details = result;
	}, reason => {
		if (!destroyed) error = String(reason);
	});

	$: item = details?.workshop ?? workshop;
	$: media = [...(item.previewUrl ? [{ type: 'image', value: item.previewUrl }] : []), ...(details?.previews ?? [])];

	function onKeydown(event) {
		// The key press that opened this also reaches the window, already handled.
		if (event.defaultPrevented) return;
		if (event.key === 'Escape' || (event.key === ' ' && !event.repeat && !event.target.closest?.('button, a, input'))) {
			event.preventDefault();
			onClose();
		}
	}
</script>

<svelte:window on:keydown={onKeydown}/>

<Modal active={true} cancel={onClose} padding="1.5rem">
	<div class="quick-look" role="dialog" aria-modal="true" aria-labelledby="quick-look-title">
		<h2 id="quick-look-title" class="select">{item.title}</h2>
		<BrowseGallery {media}/>
		<div class="description scrollbar">
			{#if error}
				<p class="error" role="alert">{error}</p>
			{:else if !details}
				<p class="status" role="status"><Loading inline size="1rem"/> {$_('browse_loading_item')}</p>
			{:else if item.description}
				<BBCode value={item.description}/>
			{/if}
		</div>
		<div class="actions">
			<button type="button" on:click={onClose}>{$_('close')}</button>
			<button type="button" class="primary" on:click={onOpen}><Eye size="1rem"/>{$_('open')}</button>
		</div>
	</div>
</Modal>

<style>
	.quick-look {
		display: flex;
		flex-direction: column;
		width: min(38rem, 80vw);
	}
	h2 {
		margin: 0 0 .75rem;
		font-size: 1.2rem;
		line-height: 1.25;
		overflow-wrap: anywhere;
	}
	.quick-look :global(.stage) {
		max-height: 42vh;
	}
	.description {
		max-height: min(11rem, 22vh);
		margin-top: .75rem;
		overflow-y: auto;
		line-height: 1.5;
	}
	.description p {
		margin: 0;
	}
	.status {
		display: flex;
		align-items: center;
		gap: .5rem;
		color: var(--text-soft);
	}
	.error {
		color: var(--text-error-soft);
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		justify-content: flex-end;
		gap: .75rem;
		margin-top: 1rem;
	}
	button {
		display: inline-flex;
		align-items: center;
		gap: .5rem;
		padding: .65rem .85rem;
		border: 0;
		border-radius: .25rem;
		background: var(--control);
		color: var(--text);
		font: inherit;
		cursor: pointer;
	}
	button.primary {
		background: var(--neutral);
		color: #fff;
	}
	button:hover {
		filter: brightness(1.15);
	}
	button:focus-visible {
		outline: 2px solid var(--text);
		outline-offset: 2px;
	}
</style>
