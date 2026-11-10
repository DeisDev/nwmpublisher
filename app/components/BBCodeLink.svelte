<script>
	import { _ } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import { taskError } from '../transactions.js';

	export let href;

	async function follow(event) {
		if (event.type === 'auxclick' && event.button !== 1) return;
		event.preventDefault();
		try {
			await invoke('open_url', { url: href });
		} catch (reason) {
			taskError($_('bbcode.link_failed', { values: { error: String(reason) } }));
		}
	}
</script>

<a {href} title={href} target="_blank" rel="noopener noreferrer" on:click={follow} on:auxclick={follow}><slot/></a>

<style>
	a {
		color: var(--link);
		text-decoration: underline;
		text-underline-offset: 3px;
	}
	a:focus-visible {
		outline: 2px solid #127cff;
		outline-offset: 2px;
	}
</style>
