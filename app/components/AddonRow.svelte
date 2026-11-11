<script>
	import { _, locale } from 'svelte-i18n';
	import { onMount } from 'svelte';
	import Check from '@lucide/svelte/icons/check';
	import { registerContext } from './ContextMenu.svelte';
	import QuickSubscribe from './QuickSubscribe.svelte';
	import { formatDate, formatPercent, formatSize } from '../format.js';
	import { ADDON_TYPES, KIND_TAGS, authors, subscribedIds, tagLabel } from '../browse.js';

	export let workshop;
	export let columns = [];
	export let template = '';
	export let onClick = null;
	export let contextItems = null;
	export let selecting = false;
	export let selected = false;
	export let badge = null;
	export let quickSubscribe = false;

	let row;
	onMount(() => registerContext(row, () => [Promise.resolve(workshop), Promise.resolve(null), false, contextItems?.()]));

	$: numberLocale = $locale === 'kr' ? 'ko' : ($locale ?? 'en');
	$: types = workshop.tags.filter(tag => ADDON_TYPES.some(type => type.toLowerCase() === tag.toLowerCase()));
	$: otherTags = workshop.tags.filter(tag => !KIND_TAGS.has(tag.toLowerCase()) && !types.includes(tag));
	$: kindLabel = workshop.kind === 'addon' && types.length ? types.map(tagLabel).join(', ') : $_('browse_kind_' + (workshop.kind ?? 'addon'));
	$: author = $authors.get(workshop.steamid64);
	$: subscribed = $subscribedIds.has(String(workshop.id));
</script>

<div class="addon-row" class:selected class:selecting style:grid-template-columns={template} bind:this={row} on:click={event => onClick?.(event, Promise.resolve(workshop), Promise.resolve(null))} role="presentation">
	<span class="select-toggle" data-select-toggle aria-hidden="true"><Check size=".8rem" strokeWidth={3}/></span>
	<span class="title">
		{#if workshop.previewUrl}<img src={workshop.previewUrl} alt="" loading="lazy"/>{:else}<span class="image-placeholder"></span>{/if}
		<span class="text select">{workshop.title}</span>
	</span>
	{#each columns as column}
		<span class={column}>
			{#if column === 'author'}{author?.name ?? (author === null ? workshop.steamid64 : '…')}
			{:else if column === 'type'}{kindLabel}
			{:else if column === 'tags'}{otherTags.map(tagLabel).join(', ')}
			{:else if column === 'size'}{workshop.fileSize == null ? '' : formatSize(workshop.fileSize, $locale)}
			{:else if column === 'subscribers'}{workshop.subscriptions.toLocaleString(numberLocale)}
			{:else if column === 'favorites'}{workshop.favorites?.toLocaleString(numberLocale) ?? ''}
			{:else if column === 'rating'}{formatPercent(workshop.score ?? 0, $locale)}
			{:else if column === 'created'}{workshop.timeCreated ? formatDate(workshop.timeCreated, $locale) : ''}
			{:else if column === 'updated'}{workshop.timeUpdated ? formatDate(workshop.timeUpdated, $locale) : ''}
			{:else if column === 'status'}{#if badge}<span class="badge">{badge}</span>{/if}
			{/if}
		</span>
	{/each}
	{#if quickSubscribe}<span class="quick">{#if !selecting}<QuickSubscribe {workshop}/>{:else if subscribed}<span class="subscribed-mark" title={$_('browse_badge_subscribed')}><Check size="1rem" strokeWidth={3}/></span>{/if}</span>{/if}
</div>

<style>
	.addon-row {
		display: grid;
		align-items: center;
		gap: .75rem;
		padding: .4rem .75rem;
		border-bottom: 1px solid var(--border);
		cursor: pointer;
		font-size: .9rem;
	}
	.addon-row:hover {
		background-color: var(--bg-card);
	}
	.addon-row.selected, .addon-row.selected:hover {
		background-color: hsl(var(--neutral-custom), var(--neutral-custom-l), .18);
		box-shadow: inset 0 0 0 2px var(--neutral);
	}
	.addon-row > span {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.title {
		display: flex;
		align-items: center;
		gap: .6rem;
	}
	.addon-row > .quick {
		display: flex;
		overflow: visible;
	}
	.subscribed-mark {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 1.75rem;
		height: 1.75rem;
		border-radius: 4px;
		background: var(--success);
		color: #fff;
	}
	.text {
		overflow: hidden;
		text-overflow: ellipsis;
	}
	img, .image-placeholder {
		width: 2rem;
		height: 2rem;
		flex-shrink: 0;
		border-radius: 3px;
		object-fit: cover;
		background: var(--bg-sunken-2);
	}
	.subscribers, .favorites, .rating, .size {
		text-align: end;
		font-variant-numeric: tabular-nums;
	}
	.author, .tags, .created, .updated {
		color: var(--text-soft);
	}
	.select-toggle {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 1.2rem;
		height: 1.2rem;
		border: 2px solid var(--border-strong);
		border-radius: 4px;
		color: transparent;
		opacity: 0;
	}
	.addon-row:hover .select-toggle, .addon-row.selecting .select-toggle, :global(:focus-visible) > .addon-row .select-toggle {
		opacity: 1;
	}
	.addon-row.selected .select-toggle {
		border-color: var(--neutral);
		background-color: var(--neutral);
		color: #fff;
	}
	.addon-row.selecting .select {
		user-select: none;
	}
	.badge {
		padding: .1rem .45rem;
		border-radius: 3px;
		background: var(--neutral-dark);
		color: #fff;
		font-size: .75rem;
	}
</style>
