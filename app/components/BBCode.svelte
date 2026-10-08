<script>
	import { parseBBCode } from '../bbcode';
	import BBCodePreview from './BBCodePreview.svelte';

	export let value = '';
	export let widgets = false;
	$: nodes = parseBBCode(value);
</script>

<div class="bbcode select"><BBCodePreview {nodes} {widgets}/></div>

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
</style>
