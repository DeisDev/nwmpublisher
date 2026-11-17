<script>
	import { settings } from '../settings.js';

	export let src;
	export let alt = '';
	export let fit = 'cover';
	export let onLoad = null;
	export let onError = null;

	let image;
	let canvas;
	let still = false;

	$: hoverOnly = !!$settings.animate_previews_on_hover;
	$: src, hoverOnly, still = false;
	$: if (hoverOnly && canvas && image?.complete) drawStill();

	function drawStill() {
		if (!image.naturalWidth) return;
		const longest = Math.max(image.naturalWidth, image.naturalHeight);
		const scale = Math.min(1, Math.max(image.clientWidth, image.clientHeight) * devicePixelRatio / longest) || 1;
		canvas.width = Math.max(1, Math.round(image.naturalWidth * scale));
		canvas.height = Math.max(1, Math.round(image.naturalHeight * scale));
		canvas.getContext('2d').drawImage(image, 0, 0, canvas.width, canvas.height);
		still = true;
	}

	function loaded(event) {
		if (hoverOnly && canvas) drawStill();
		onLoad?.(event);
	}
</script>

<span class="preview-image" class:still>
	<img bind:this={image} {src} {alt} loading="lazy" style:object-fit={fit} on:load={loaded} on:error={event => onError?.(event)}/>
	{#if hoverOnly}<canvas bind:this={canvas} style:object-fit={fit} aria-hidden="true"></canvas>{/if}
</span>

<style>
	.preview-image {
		position: relative;
		display: block;
		width: 100%;
		height: 100%;
	}
	img, canvas {
		display: block;
		width: 100%;
		height: 100%;
	}
	canvas {
		position: absolute;
		inset: 0;
		visibility: hidden;
	}
	.still > img {
		visibility: hidden;
	}
	.still > canvas {
		visibility: visible;
	}
	:global([data-preview-host]:hover) .still > img, :global([data-preview-host]:focus-within) .still > img, :global(:focus-visible > [data-preview-host]) .still > img {
		visibility: visible;
	}
	:global([data-preview-host]:hover) .still > canvas, :global([data-preview-host]:focus-within) .still > canvas, :global(:focus-visible > [data-preview-host]) .still > canvas {
		visibility: hidden;
	}
</style>
