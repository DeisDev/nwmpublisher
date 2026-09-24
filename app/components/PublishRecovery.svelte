<script>
	import { onMount } from 'svelte';
	import { _ } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import { open } from '@tauri-apps/plugin-shell';
	import { confirm } from '@tauri-apps/plugin-dialog';
	import { translateError } from '../i18n';
	let operations = [];
	let error = null;
	let evidence = {};
	let chosenIds = {};
	let busy = false;
	async function load() {
		try { operations = await invoke('publish_operations'); }
		catch (failure) { error = translateError(String(failure)); }
	}
	onMount(() => { load(); const timer = setInterval(load, 10000); return () => clearInterval(timer); });
	async function recheck(operation) {
		busy = true;
		try { evidence[operation.key] = await invoke('reconcile_publish', { key: operation.key }); error = null; }
		catch (failure) { error = translateError(String(failure)); }
		finally { busy = false; }
	}
	async function reviewed(operation) {
		if (!await confirm($_('publish_recovery_confirm'), { title: $_('publish_recovery_title'), kind: 'warning' })) return;
		busy = true;
		try {
			await invoke('review_publish', { key: operation.key, workshopId: String(operation.workshopId ?? chosenIds[operation.key] ?? '') });
			await load();
			error = null;
		} catch (failure) { error = translateError(String(failure)); }
		finally { busy = false; }
	}
</script>

{#if operations.some(operation => !operation.reviewed) || error}
	<details class="publish-recovery">
		<summary>{$_('publish_recovery_title')}</summary>
		{#if error}<p role="alert">{error}</p>{/if}
		{#each operations.filter(operation => !operation.reviewed) as operation (operation.key)}
			<section>
				<p>{$_(operation.outcome === 'published' ? 'publish_cleanup_warning' : 'publish_outcome_unknown')}</p>
				{#if operation.staging}<code>{operation.staging}</code>{/if}
				{#if operation.workshopId}
					<button on:click={() => open(`https://steamcommunity.com/sharedfiles/filedetails/?id=${operation.workshopId}`).catch(failure => error = String(failure))}>{$_('workshop_page')} · {operation.workshopId}</button>
					<button disabled={busy} on:click={() => recheck(operation)}>{$_('publish_recheck')}</button>
				{:else}
					<label>{$_('publish_recovery_id')}<input bind:value={chosenIds[operation.key]} inputmode="numeric"/></label>
				{/if}
				{#if evidence[operation.key]}<p>{$_('publish_remote_observed', { values: { title: evidence[operation.key].title } })}</p>{/if}
				{#if operation.outcome !== 'published'}<button disabled={busy} on:click={() => reviewed(operation)}>{$_('publish_recovery_reviewed')}</button>{/if}
				{#each operation.cleanupWarnings as warning}<p>{translateError(warning)}</p>{/each}
			</section>
		{/each}
	</details>
{/if}

<style>
	.publish-recovery { position: fixed; inset-block-start: 4.5rem; inset-inline-end: 1rem; max-width: 36rem; max-height: 65vh; overflow: auto; background: #252525; padding: .75rem; z-index: 9998; box-shadow: 0 2px 8px #0008; }
	summary, button { cursor: pointer; }
	section { border-top: 1px solid #666; margin-top: .5rem; overflow-wrap: anywhere; }
	button, input { margin: .25rem; padding: .5rem; }
</style>
