<script>
	import { _ } from 'svelte-i18n';
	import Small from '@lucide/svelte/icons/grid-3x3';
	import Large from '@lucide/svelte/icons/grid-2x2';
	import { applyCardSize, saveSettings, settings } from '../settings.js';

	$: size = $settings.addon_card_size;

	function preview(event) {
		size = Number(event.currentTarget.value);
		applyCardSize(size);
	}
</script>

<label class="card-size" title={$_('card_size')}>
	<Small size="1rem"/>
	<input type="range" min="140" max="300" step="10" value={size} aria-label={$_('card_size')} on:input={preview} on:change={() => saveSettings({ addon_card_size: size }).catch(() => {})}/>
	<Large size="1rem"/>
</label>

<style>
	.card-size {
		display: inline-flex;
		align-items: center;
		gap: .4rem;
		color: var(--text-muted);
	}
	input {
		width: 6rem;
		margin: 0;
		accent-color: var(--neutral);
		cursor: pointer;
	}
	input:focus-visible {
		outline: 2px solid var(--text);
		outline-offset: 2px;
	}
</style>
