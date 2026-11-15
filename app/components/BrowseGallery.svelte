<script>
	import { _ } from 'svelte-i18n';
	import Play from '@lucide/svelte/icons/play';
	import BBCodeWidget from './BBCodeWidget.svelte';

	export let media = [];

	let shown = 0;
	$: current = media[Math.min(shown, media.length - 1)];
</script>

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

<style>
	.gallery {
		min-width: 0;
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
		display: flex;
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
	.thumbs button:focus-visible {
		outline: 2px solid var(--text);
		outline-offset: 2px;
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
</style>
