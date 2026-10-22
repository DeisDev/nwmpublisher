<script>
	import { _ } from 'svelte-i18n';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import { formatSize } from '../format.js';
	import { workshopUrl } from '../collections.js';

	export let id;
	export let entry = null;
	export let pending = false;
	export let note = '';
</script>

<li class:pending>
	<slot name="start"></slot>
	{#if entry?.previewUrl}<img src={entry.previewUrl} alt="" loading="lazy"/>{:else}<div class="image-placeholder" aria-hidden="true">?</div>{/if}
	<div class="text">
		<a href={workshopUrl(id)} target="_blank" rel="noreferrer">{entry?.title ?? (entry ? $_('collections_unavailable_item') : id)}<ExternalLink size=".75rem"/></a>
		<small>{id}{#if entry?.fileSize} · {formatSize(entry.fileSize)}{/if}{#if entry && entry.status !== 'available'} · <span class="status">{$_('collections_status_' + entry.status)}</span>{/if}{#if note} · {note}{/if}{#if pending} · <span class="pending-label">{$_('workshop_pending_addition')}</span>{/if}</small>
	</div>
	<slot></slot>
</li>

<style>
	li { display: flex; align-items: center; gap: .75rem; padding: .6rem .75rem; border-inline-start: 3px solid transparent; background: var(--bg-preview); border-radius: 4px; margin-bottom: .5rem; }
	li.pending { border-inline-start-color: var(--neutral); }
	img, .image-placeholder { width: 2.6rem; height: 2.6rem; border-radius: 4px; flex-shrink: 0; object-fit: cover; background: var(--control); }
	.image-placeholder { display: grid; place-items: center; color: var(--text-muted); }
	.text { min-width: 0; flex: 1; }
	a { display: inline-flex; align-items: center; gap: .3rem; color: var(--link); overflow-wrap: anywhere; font-size: .9em; text-decoration: none; }
	a:hover { text-decoration: underline; }
	a:focus-visible { outline: 2px solid #127cff; outline-offset: 1px; }
	small { display: block; color: var(--text-muted); font-size: .8em; margin-top: .25rem; overflow-wrap: anywhere; }
	.status { color: var(--text-error-soft); }
	.pending-label { color: var(--text); }
</style>
