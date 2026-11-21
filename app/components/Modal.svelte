<script>
	import { onDestroy, tick } from 'svelte';
	import { closeDialog, openDialog } from '../modals.js';

	export let id = null;
	export let active = false;
	export let cancel = null;
	export let padding = null;
	export let labelledby = null;

	let dialog;

	$: if (dialog) active ? show() : hide();

	function show() {
		if (dialog.open) return;
		dialog.showModal();
		openDialog(dialog);
	}

	function hide() {
		if (dialog.open) dialog.close();
	}

	function requestCancel(event) {
		event.preventDefault();
		cancel?.();
	}

	function clickOutside(event) {
		if (event.target === dialog) cancel?.();
	}

	// Browsers may close a modal despite a prevented cancel, for example on repeated Escape presses.
	async function closed() {
		closeDialog(dialog);
		if (!active) return;
		cancel?.();
		await tick();
		if (active && dialog && !dialog.open) show();
	}

	onDestroy(() => {
		if (dialog) closeDialog(dialog);
	});
</script>

<dialog {id} bind:this={dialog} aria-labelledby={labelledby} on:cancel={requestCancel} on:close={closed} on:click={clickOutside}>
	<div class="hide-scroll" style={padding ? ('padding:' + padding) : null}>
		<slot></slot>
	</div>
</dialog>

<style>
	dialog {
		position: fixed;
		inset: 0;
		width: 100%;
		height: 100%;
		max-width: none;
		max-height: none;
		margin: 0;
		padding: 0;
		border: 0;
		overflow: hidden;
		overscroll-behavior: contain;
		color: inherit;
		background: transparent;

		transition-property: display, overlay;
		transition-duration: .25s;
		transition-behavior: allow-discrete;
	}
	dialog::backdrop {
		overflow: hidden;
		overscroll-behavior: contain;
		background-color: rgba(0,0,0,0);

		transition-property: backdrop-filter, background-color, display, overlay;
		transition-duration: .25s;
		transition-behavior: allow-discrete;
	}
	dialog[open]::backdrop {
		backdrop-filter: grayscale(.5) blur(1px);
		background-color: rgba(0,0,0,.4);
	}
	@starting-style {
		dialog[open]::backdrop {
			backdrop-filter: none;
			background-color: rgba(0,0,0,0);
		}
	}
	dialog > div {
		position: absolute;
		top: 0;
		left: 0;
		bottom: 0;
		right: 0;
		margin: auto;

		background-color: var(--bg-app);
		border-radius: .3rem;
		box-shadow: 0 0 10px rgba(0, 0, 0, .25);

		width: min-content;
		height: min-content;
		max-width: 90%;
		max-height: 90%;
	}
	@media (prefers-reduced-motion: no-preference) {
		dialog > div {
			transition: transform .25s;
			transform: scale(0, 0);
		}
		dialog[open] > div {
			transform: scale(1, 1);
		}
		@starting-style {
			dialog[open] > div {
				transform: scale(0, 0);
			}
		}
	}
	@media (prefers-reduced-motion: reduce) {
		dialog, dialog::backdrop {
			transition-duration: .1s;
		}
	}
</style>
