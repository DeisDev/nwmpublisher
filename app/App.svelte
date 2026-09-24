<script>
	import { _ } from 'svelte-i18n';
	import TasksOverlay from './components/TasksOverlay.svelte';
	import Navbar from './components/Navbar.svelte';
	import NavSidebar, { pages, activeItem } from './components/NavSidebar.svelte';
	import GitHubStar from './components/GitHubStar.svelte';
	import ContextMenuContainer from './components/ContextMenuContainer.svelte';
	import MigrateSettings from './components/MigrateSettings.svelte';
	import RecoverSettings from './components/RecoverSettings.svelte';
	import PublishRecovery from './components/PublishRecovery.svelte';
	import { settingsSave } from './settings.js';
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import { Transaction } from './transactions.js';

	onMount(() => {
		const recover = ({ detail: snapshot }) => {
			if (snapshot.context?.kind !== 'publish') return;
			new Transaction(snapshot.id, transaction => transaction.status ? $_(transaction.status) : $_('publish_recovered')).applySnapshot(snapshot);
		};
		window.addEventListener('recovered-job', recover);
		return () => window.removeEventListener('recovered-job', recover);
	});

	let migrateSettings = false;
	let recovery = null;
	invoke('settings_recovery').then(value => recovery = value);
	invoke('legacy_settings_pending').then(pending => migrateSettings = pending);
</script>

<main>

	<TasksOverlay/>
	<PublishRecovery/>
	{#if $settingsSave.state === 'failed'}
		<div class="settings-error" role="alert">{$_('settings_save_failed')}: {$settingsSave.error}</div>
	{/if}
	{#if recovery}<RecoverSettings {recovery}/>{/if}

	<ContextMenuContainer/>

	<Navbar/>

	<NavSidebar/>

	<div id="content">
		{#if !$pages[$activeItem].persist && !$pages[$activeItem].background}
			<svelte:component this={$pages[$activeItem].component}/>
		{/if}
		{#each $pages as page, i}
			{#if page.background || (page.persist && page.created)}
				<div class="persist" class:active={$activeItem == i}><svelte:component this={page.component}/></div>
			{/if}
		{/each}
	</div>

	{#if AppData.open_count === 5}
		<GitHubStar/>
	{/if}

	{#if migrateSettings}
		<MigrateSettings bind:active={migrateSettings}/>
	{/if}

</main>

<style>
	.settings-error { position: fixed; inset: auto 1rem 1rem; z-index: 10000; padding: .75rem; background: var(--error); }
	/*
	#file-drop {
		position: absolute;
		top: 0;
		left: 0;
		width: 100%;
		height: 100%;
		opacity: 0;
		display: flex;
		justify-content: center;
		align-items: center;
		background-color: rgba(0,0,0,.4);
		backdrop-filter: grayscale(0) blur(0px);
		pointer-events: none;
		-webkit-pointer-events: none;
		transition: opacity .25s;
		z-index: 9999;
	}
	:global(body.file-drop #file-drop) {
		opacity: 1 !important;
		backdrop-filter: grayscale(.5) blur(1px) !important;
	}
	#file-drop :global(.icon) {
		width: min(50vw, 50vh);
	}
	*/

	main {
		width: 100%;
		height: 100%;
	}

	#content {
		width: 100%;
		height: 100%;
		padding-top: 70px;
		padding-left: min(26.04%, 250px);
	}
	#content .persist {
		height: 100%;
	}
	#content .persist:not(.active) {
		display: none;
	}
</style>
