<script>
	import { _ } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import Modal from './Modal.svelte';

	export let active = false;
	let errorMessage = null;

	function importSettings() {
		errorMessage = null;
		invoke('migrate_legacy_settings').then(() => {
			// Every part of the UI reads the settings when it mounts, so restart the webview
			// instead of trying to refresh them one by one
			window.location.reload();
		}).catch(error => errorMessage = String(error));
	}

	function startFresh() {
		errorMessage = null;
		invoke('dismiss_legacy_settings').then(() => {
			active = false;
		}).catch(error => errorMessage = String(error));
	}

	// Dismissing without choosing leaves the old settings alone, so the offer comes back
	// on the next launch
	function dismiss() {
		active = false;
	}
</script>

<Modal id="migrate-settings" {active} cancel={dismiss}>
	<h2>{$_('migrate_settings_title')}</h2>
	<p>{$_('migrate_settings_body')}</p>
	{#if errorMessage}
		<p role="alert">{errorMessage}</p>
	{/if}
	<div class="buttons">
		<div class="btn primary" on:click={importSettings}>{$_('migrate_settings_import')}</div>
		<div class="btn" on:click={startFresh}>{$_('migrate_settings_fresh')}</div>
	</div>
</Modal>

<style>
	:global(#migrate-settings > .hide-scroll) {
		padding: 1.5rem;
		width: 26rem;
		text-align: center;
	}

	h2 {
		margin-top: 0;
	}

	p {
		white-space: pre-line;
		line-height: 1.6;
	}

	.buttons {
		display: flex;
		margin-top: 1.5rem;
	}

	.btn {
		cursor: pointer;
		flex: 1;
		background: #313131;
		box-shadow: 0px 0px 2px 0px rgb(0 0 0 / 40%);
		border-radius: 4px;
		padding: .7rem;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.btn:active {
		background: #252525;
	}
	.btn.primary {
		background-color: var(--neutral);
		margin-right: .75rem;
	}
	.btn.primary:active {
		background: #252525;
	}
</style>
