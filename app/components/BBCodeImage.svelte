<script>
	import { _ } from 'svelte-i18n';

	export let source;
	export let sourceStart = undefined;
	let failed = false;
	let src;
	$: {
		src = imageUrl(source);
		failed = false;
	}

	function imageUrl(value) {
		let url;
		try {
			url = new URL(value.trim());
		} catch (error) {
			if (error instanceof TypeError) return null;
			throw error;
		}
		return url.protocol === 'https:' || url.protocol === 'http:' ? url.href : null;
	}
</script>

{#key src}
	{#if !src}
		<span class="image-error" title={source} data-source-start={sourceStart}>{$_('bbcode.image_invalid')}</span>
	{:else if failed}
		<span class="image-error" title={src} data-source-start={sourceStart}>{$_('bbcode.image_failed')}</span>
	{:else}
		<img {src} data-source-start={sourceStart} alt={$_('bbcode.img')} title={src} decoding="async" referrerpolicy="no-referrer" on:error={() => failed = true}/>
	{/if}
{/key}

<style>
	img {
		width: auto;
		height: auto;
		max-width: 100%;
		max-height: min(20rem, 35vh);
		object-fit: contain;
		vertical-align: middle;
	}
	.image-error {
		display: inline-block;
		max-width: 100%;
		padding: .4rem .7rem;
		border: 1px dashed var(--border-muted);
		border-radius: 4px;
		color: var(--text-muted);
		font-size: .85em;
		white-space: normal;
	}
</style>
