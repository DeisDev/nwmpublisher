<script>
	import { _ } from 'svelte-i18n';
	import { onDestroy } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import BBCodeImage from './BBCodeImage.svelte';
	import BBCodeLink from './BBCodeLink.svelte';
	import Loading from './Loading.svelte';

	export let widget;
	export let href;
	export let sourceStart = undefined;
	let timer;
	$: workshopId = widget.type === 'workshop' ? widget.id : null;
	$: item = loadWorkshop(workshopId);
	onDestroy(() => clearTimeout(timer));

	function loadWorkshop(id) {
		clearTimeout(timer);
		return id ? new Promise(resolve => {
			timer = setTimeout(() => resolve(invoke('preview_workshop_item', { itemId: id })), 300);
		}) : null;
	}
</script>

<span class="widget" data-source-start={sourceStart}>
	{#if widget.type === 'workshop'}
		<span class="workshop">
			{#await item}
				<Loading size="1.5rem"/>
			{:then addon}
				<BBCodeLink {href}><span class="card">{#if addon.previewUrl}<span class="thumbnail"><BBCodeImage source={addon.previewUrl}/></span>{/if}<span><strong>{addon.title}</strong><small>{$_('steam_workshop')}</small></span></span></BBCodeLink>
			{:catch error}
				<span role="alert">{$_('bbcode.widget_failed', { values: { error: String(error) } })}</span>
			{/await}
		</span>
	{:else}
		{#key widget.src}
			<iframe src={widget.src} title={$_('bbcode.widget_' + widget.type)} class:youtube={widget.type === 'youtube'} loading="lazy" sandbox="allow-scripts allow-same-origin allow-presentation" allow="encrypted-media; fullscreen; picture-in-picture" referrerpolicy="strict-origin-when-cross-origin"></iframe>
		{/key}
	{/if}
	<small><BBCodeLink {href}>{href}</BBCodeLink></small>
</span>

<style>
	.widget {
		display: block;
		width: 100%;
		max-width: 40rem;
		margin-block: .5rem;
		white-space: normal;
	}
	iframe {
		display: block;
		width: 100%;
		height: 190px;
		border: 0;
	}
	iframe.youtube {
		height: auto;
		min-height: 200px;
		min-width: 200px;
		aspect-ratio: 16 / 9;
	}
	.workshop {
		display: block;
		padding: .7rem;
		border: 1px solid var(--border);
		border-radius: 4px;
		background: var(--bg-subtle);
	}
	.card {
		display: flex;
		align-items: center;
		gap: .75rem;
	}
	.thumbnail {
		width: 5rem;
		flex-shrink: 0;
	}
	small {
		display: block;
		margin-top: .35rem;
		color: var(--text-muted);
	}
</style>
