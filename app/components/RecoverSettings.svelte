<script>
	import { _ } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import Modal from './Modal.svelte';
	export let recovery;
	let error = null;
	let busy = false;
	async function recover(useBackup) {
		busy = true;
		try {
			await invoke('recover_settings', { useBackup });
			window.location.reload();
		} catch (failure) { error = String(failure); busy = false; }
	}
</script>

<Modal id="recover-settings" active={true} cancel={() => {}}>
	<h2>{$_('settings_recovery_title')}</h2>
	<p>{$_('settings_recovery_body')}</p>
	<p>{recovery[0]}</p>
	{#if error}<p role="alert">{error}</p>{/if}
	<button disabled={busy || !recovery[1]} on:click={() => recover(true)}>{$_('settings_recovery_backup')}</button>
	<button disabled={busy} on:click={() => recover(false)}>{$_('settings_recovery_defaults')}</button>
</Modal>

<style>
	:global(#recover-settings > div) { padding: 1.5rem; max-width: 36rem; }
	p { overflow-wrap: anywhere; }
	button { margin: .5rem; padding: .75rem; }
</style>
