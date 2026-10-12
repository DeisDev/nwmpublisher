<script>
	import { parseBBCode } from '../bbcode';
	import BBCodePreview from './BBCodePreview.svelte';

	export let value = '';
	export let widgets = false;
	export let sourceMap = false;
	export let steam = false;
	$: nodes = parseBBCode(value);
</script>

<div class="bbcode select" class:steam><BBCodePreview {nodes} {widgets} {sourceMap} {steam}/></div>

<style>
	.bbcode {
		min-width: 0;
		line-height: 1.5;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	.bbcode :global(*) {
		user-select: text;
	}
	.steam {
		font: 14px/20px "Motiva Sans", Arial, Helvetica, sans-serif;
	}
</style>
