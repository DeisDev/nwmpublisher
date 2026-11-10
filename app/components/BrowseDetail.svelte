<script>
	import { _, locale } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import { onDestroy } from 'svelte';
	import ThumbsUp from '@lucide/svelte/icons/thumbs-up';
	import ThumbsDown from '@lucide/svelte/icons/thumbs-down';
	import Star from '@lucide/svelte/icons/star';
	import CirclePlus from '@lucide/svelte/icons/circle-plus';
	import CircleMinus from '@lucide/svelte/icons/circle-minus';
	import CloudDownload from '@lucide/svelte/icons/cloud-download';
	import ListPlus from '@lucide/svelte/icons/list-plus';
	import LinkOut from '@lucide/svelte/icons/external-link';
	import LinkChain from '@lucide/svelte/icons/link';
	import Play from '@lucide/svelte/icons/play';
	import Loading from './Loading.svelte';
	import BBCode from './BBCode.svelte';
	import BBCodeWidget from './BBCodeWidget.svelte';
	import WorkshopStats from './WorkshopStats.svelte';
	import CollectionItemRow from './CollectionItemRow.svelte';
	import BrowseCollection from './BrowseCollection.svelte';
	import { formatDate, formatPercent, formatSize } from '../format.js';
	import { taskError, taskMessage } from '../transactions.js';
	import { playSound } from '../sounds.js';
	import { chooseCollections, collections, ensureCollections, workshopUrl } from '../collections.js';
	import { KIND_TAGS, authors, browseJob, downloadItems, favoriteIds, loadFavorites, requestAuthor, setFavorite, setVote, subscribeItems, subscribedIds, tagLabel, unsubscribeItems } from '../browse.js';

	export let reference;
	export let onOpen;
	export let onAuthor;
	export let onLoaded = null;

	let details = null;
	let error = null;
	let loading = true;
	let shown = 0;
	let voting = false;
	let favoriting = false;
	let destroyed = false;
	onDestroy(() => destroyed = true);

	$: load(reference);
	$: workshop = details?.workshop;
	$: id = workshop ? String(workshop.id) : null;
	$: collection = workshop?.kind === 'collection';
	$: media = workshop ? [...(workshop.previewUrl ? [{ type: 'image', value: workshop.previewUrl }] : []), ...details.previews] : [];
	$: current = media[Math.min(shown, media.length - 1)];
	$: author = workshop && $authors.get(workshop.steamid64);
	$: tags = workshop ? workshop.tags.filter(tag => !KIND_TAGS.has(tag.toLowerCase())) : [];
	$: subscribed = id && $subscribedIds.has(id);
	$: favorite = id && !!$favoriteIds?.has(id);
	$: memberOf = id ? $collections.items.filter(item => item.fields.items.includes(id)) : [];

	async function load(reference) {
		loading = true;
		error = null;
		details = null;
		shown = 0;
		try {
			const result = await invoke('browse_item', { reference });
			if (destroyed || reference !== currentReference()) return;
			details = result;
			requestAuthor(result.workshop.steamid64);
			onLoaded?.(result.workshop);
		} catch (err) {
			if (!destroyed && reference === currentReference()) error = String(err);
		} finally {
			if (!destroyed && reference === currentReference()) loading = false;
		}
	}
	const currentReference = () => reference;

	ensureCollections();
	loadFavorites();

	async function vote(up) {
		voting = true;
		if (await setVote(id, up)) details = { ...details, vote: up ? 'up' : 'down' };
		voting = false;
	}

	async function toggleFavorite() {
		favoriting = true;
		await setFavorite(id, !favorite);
		favoriting = false;
	}

	async function copyLink() {
		try {
			await navigator.clipboard.writeText(workshopUrl(id));
		} catch (err) {
			taskError($_('links_copy_failed', { values: { error: String(err) } }));
			return;
		}
		taskMessage($_('links_copied', { values: { count: 1 } }));
		playSound('success');
	}
</script>

<div class="detail scrollbar">
	{#if loading}
		<p class="status" role="status"><Loading inline size="1rem"/> {$_('browse_loading_item')}</p>
	{:else if error}
		<div class="status error" role="alert">
			<p>{error}</p>
			<button type="button" on:click={() => load(reference)}>{$_('workshop_retry')}</button>
		</div>
	{:else if workshop}
		<div class="top">
			<div class="gallery">
				<div class="stage">
					{#if !current}
						<div class="empty"></div>
					{:else if current.type === 'youtube'}
						<BBCodeWidget widget={{ type: 'youtube', src: 'https://www.youtube.com/embed/' + current.value }} href={'https://www.youtube.com/watch?v=' + current.value}/>
					{:else}
						<img src={current.value} alt=""/>
					{/if}
				</div>
				{#if media.length > 1}
					<div class="thumbs scrollbar">
						{#each media as item, index}
							<button type="button" class:active={index === shown} aria-label={$_(item.type === 'youtube' ? 'browse_show_video' : 'browse_show_image', { values: { number: index + 1 } })} on:click={() => shown = index}>
								<img src={item.type === 'youtube' ? `https://i.ytimg.com/vi/${item.value}/default.jpg` : item.value} alt="" loading="lazy"/>
								{#if item.type === 'youtube'}<span class="play"><Play size="1rem"/></span>{/if}
							</button>
						{/each}
					</div>
				{/if}
			</div>
			<div class="summary">
				<h2 class="select">{workshop.title}</h2>
				<p class="author">
					{$_('browse_by', { values: { name: author?.name ?? workshop.steamid64 } })}
					{#if workshop.steamid64}<button type="button" class="plain" on:click={() => onAuthor({ id: workshop.steamid64, name: author?.name ?? null })}>{$_('browse_more_by_author')}</button>{/if}
				</p>
				{#if tags.length}
					<div class="tags">{#each tags as tag}<span class="chip">{tagLabel(tag)}</span>{/each}</div>
				{/if}
				<dl class="facts">
					{#if collection}
						<div><dt>{$_('browse_items')}</dt><dd>{(workshop.numChildren ?? 0).toLocaleString($locale === 'kr' ? 'ko' : $locale)}</dd></div>
						{#if details.totalSize != null}<div><dt>{$_('browse_total_size')}</dt><dd>{formatSize(details.totalSize, $locale)}</dd></div>{/if}
					{:else if workshop.fileSize != null}
						<div><dt>{$_('browse_column_size')}</dt><dd>{formatSize(workshop.fileSize, $locale)}</dd></div>
					{/if}
					<div><dt>{$_('browse_column_rating')}</dt><dd>{formatPercent(workshop.score ?? 0, $locale)}</dd></div>
					{#if workshop.timeCreated}<div><dt>{$_('browse_column_created')}</dt><dd>{formatDate(workshop.timeCreated, $locale)}</dd></div>{/if}
					{#if workshop.timeUpdated}<div><dt>{$_('browse_column_updated')}</dt><dd>{formatDate(workshop.timeUpdated, $locale)}</dd></div>{/if}
				</dl>
				<div class="actions">
					{#if !collection}
						{#if subscribed}
							<button type="button" disabled={!!$browseJob} on:click={() => unsubscribeItems([{ id, title: workshop.title }])}><CircleMinus size="1rem"/>{$_('unsubscribe')}</button>
						{:else}
							<button type="button" class="primary" disabled={!!$browseJob} on:click={() => subscribeItems([id])}><CirclePlus size="1rem"/>{$_('browse_subscribe')}</button>
						{/if}
						{#if workshop.kind === 'addon'}
							<button type="button" on:click={() => downloadItems([id])}><CloudDownload size="1rem"/>{$_('download')}</button>
						{/if}
						<button type="button" on:click={() => chooseCollections([id])}><ListPlus size="1rem"/>{$_('collections_menu')}</button>
					{/if}
					<button type="button" class="icon" class:active={details.vote === 'up'} aria-pressed={details.vote === 'up'} disabled={voting} title={$_('browse_vote_up')} aria-label={$_('browse_vote_up')} on:click={() => vote(true)}><ThumbsUp size="1rem"/></button>
					<button type="button" class="icon" class:active={details.vote === 'down'} aria-pressed={details.vote === 'down'} disabled={voting} title={$_('browse_vote_down')} aria-label={$_('browse_vote_down')} on:click={() => vote(false)}><ThumbsDown size="1rem"/></button>
					<button type="button" class="icon" class:active={favorite} aria-pressed={favorite} disabled={favoriting || !$favoriteIds} title={$_(favorite ? 'browse_unfavorite' : 'browse_favorite')} aria-label={$_(favorite ? 'browse_unfavorite' : 'browse_favorite')} on:click={toggleFavorite}><Star size="1rem"/></button>
					<button type="button" class="icon" title={$_('copy_link')} aria-label={$_('copy_link')} on:click={copyLink}><LinkChain size="1rem"/></button>
				</div>
				<p class="links">
					<a href={workshopUrl(id)} target="_blank" rel="noreferrer">{$_('steam_workshop')}<LinkOut size=".8rem"/></a>
					<a href={'https://steamcommunity.com/sharedfiles/filedetails/comments/' + id} target="_blank" rel="noreferrer">{$_('browse_comments')}<LinkOut size=".8rem"/></a>
					{#if !collection}<a href={'https://steamcommunity.com/sharedfiles/filedetails/changelog/' + id} target="_blank" rel="noreferrer">{$_('browse_change_notes')}<LinkOut size=".8rem"/></a>{/if}
				</p>
			</div>
		</div>

		<WorkshopStats item={workshop}/>

		{#if collection}
			<BrowseCollection {workshop} children={details.children} {onOpen}/>
		{/if}

		{#if workshop.description}
			<section>
				<h3>{$_('browse_description')}</h3>
				<div class="description"><BBCode value={workshop.description}/></div>
			</section>
		{/if}

		{#if details.dependencies.length}
			<section>
				<h3>{$_('browse_required_addons')} <span class="count">{details.dependencies.length}</span></h3>
				<ul>
					{#each details.dependencies as entry (entry.id)}
						<CollectionItemRow id={entry.id} {entry}>
							{#if entry.status === 'available'}<button type="button" on:click={() => onOpen(entry.id)}>{$_('open')}</button>{/if}
						</CollectionItemRow>
					{/each}
				</ul>
			</section>
		{/if}

		{#if memberOf.length}
			<section>
				<h3>{$_('browse_in_your_collections')} <span class="count">{memberOf.length}</span></h3>
				<p class="members">{memberOf.map(item => item.fields.title).join(', ')}</p>
			</section>
		{/if}
	{/if}
</div>

<style>
	.detail {
		height: 100%;
		overflow-y: auto;
		padding: 1rem 1.5rem 1.5rem;
	}
	.status {
		display: flex;
		align-items: center;
		gap: .5rem;
		margin: 0;
		color: var(--text-soft);
	}
	.status.error {
		flex-direction: column;
		align-items: flex-start;
		color: var(--text-error-soft);
	}
	.status p {
		margin: 0;
	}
	.top {
		display: grid;
		grid-template-columns: minmax(16rem, 2fr) minmax(16rem, 3fr);
		gap: 1.5rem;
		margin-bottom: 1rem;
	}
	@media (max-width: 900px) {
		.top {
			grid-template-columns: minmax(0, 1fr);
		}
	}
	.stage {
		aspect-ratio: 16 / 9;
		display: flex;
		align-items: center;
		justify-content: center;
		overflow: hidden;
		border-radius: 4px;
		background: var(--bg-sunken-2);
		box-shadow: 0 0 2px 1px rgba(0, 0, 0, .5);
	}
	.stage img {
		max-width: 100%;
		max-height: 100%;
		object-fit: contain;
	}
	.stage :global(.widget), .stage :global(iframe) {
		width: 100%;
		height: 100%;
		border: 0;
	}
	.thumbs {
		display: flex;
		gap: .5rem;
		margin-top: .5rem;
		overflow-x: auto;
		padding-bottom: .25rem;
	}
	.thumbs button {
		position: relative;
		flex: 0 0 4.5rem;
		height: 3.2rem;
		padding: 0;
		border: 2px solid transparent;
		border-radius: 4px;
		overflow: hidden;
		background: var(--bg-sunken-2);
		cursor: pointer;
	}
	.thumbs button.active {
		border-color: var(--neutral);
	}
	.thumbs img {
		width: 100%;
		height: 100%;
		object-fit: cover;
	}
	.play {
		position: absolute;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		color: #fff;
		background: rgb(0 0 0 / 35%);
	}
	h2 {
		margin: 0 0 .4rem;
		font-size: 1.5rem;
		line-height: 1.25;
		overflow-wrap: anywhere;
	}
	.author {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: .25rem .75rem;
		margin: 0 0 .75rem;
		color: var(--text-soft);
	}
	.tags {
		display: flex;
		flex-wrap: wrap;
		gap: .35rem;
		margin-bottom: .75rem;
	}
	.chip {
		padding: .15rem .5rem;
		border: 1px solid var(--border);
		border-radius: 4px;
		background: var(--control);
		font-size: .8rem;
	}
	.facts {
		display: flex;
		flex-wrap: wrap;
		gap: .5rem 1.5rem;
		margin: 0 0 1rem;
	}
	.facts dt {
		color: var(--text-muted);
		font-size: .8rem;
	}
	.facts dd {
		margin: 0;
		font-weight: 600;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: .5rem;
		margin-bottom: .75rem;
	}
	button {
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
	button:hover:not(:disabled) {
		background: var(--control-hover);
	}
	button:disabled {
		opacity: .45;
		cursor: default;
	}
	button.primary {
		border-color: var(--neutral);
		background: var(--neutral);
		color: #fff;
	}
	button.primary:hover:not(:disabled) {
		background: var(--neutral);
		filter: brightness(1.15);
	}
	button.icon {
		padding: .5rem;
	}
	button.icon.active {
		border-color: var(--neutral);
		background: var(--neutral-dark);
		color: #fff;
	}
	button.plain {
		padding: .2rem .4rem;
		border-color: transparent;
		background: none;
		color: var(--link);
	}
	button:focus-visible, a:focus-visible {
		outline: 2px solid var(--text);
		outline-offset: 2px;
	}
	.links {
		display: flex;
		flex-wrap: wrap;
		gap: .5rem 1.25rem;
		margin: 0;
	}
	.links a {
		display: inline-flex;
		align-items: center;
		gap: .3rem;
		color: var(--link);
	}
	section {
		margin-top: 1.25rem;
	}
	h3 {
		margin: 0 0 .6rem;
		font-size: 1rem;
	}
	.count {
		color: var(--text-muted);
		font-weight: normal;
	}
	.description {
		line-height: 1.5;
	}
	ul {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.members {
		margin: 0;
		color: var(--text-soft);
	}
</style>
