<script>
	export let click = null;
	export let disabled = false;

	function onClick(event) {
		if (disabled) event.stopPropagation();
		else click?.(event);
	}

	function onKeydown(event) {
		if (event.key !== 'Enter' && event.key !== ' ') return;
		event.preventDefault();
		event.currentTarget.click();
	}
</script>

<div class="item" role="menuitem" tabindex="-1" aria-disabled={disabled || null} class:disabled on:click={onClick} on:keydown={onKeydown}>
	<slot name="icon"></slot>
	<slot name="label"></slot>
</div>

<style>
	.item {
		display: flex;
		align-items: center;
		padding: .7rem;
		cursor: pointer;
		background-color: rgba(255,255,255,0);
		transition: background-color .1s;
	}
	.item > :global(*:not(:last-child)) {
		margin-right: .5rem;
	}
	.item:hover, .item:focus {
		background-color: var(--fill);
		outline: none;
	}
	.item.disabled {
		opacity: .45;
		cursor: default;
	}
	.item.disabled:hover {
		background-color: transparent;
	}
</style>
