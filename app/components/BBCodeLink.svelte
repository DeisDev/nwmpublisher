<script>
	import { _ } from 'svelte-i18n';
	import { open } from '@tauri-apps/plugin-shell';

	export let href;
	let error = null;
	$: if (href) error = null;

	async function follow(event) {
		if (event.type === 'auxclick' && event.button !== 1) return;
		event.preventDefault();
		const target = href;
		error = null;
		try {
			await open(target);
		} catch (reason) {
			if (target === href) error = String(reason);
		}
	}
</script>

<a {href} title={href} target="_blank" rel="noopener noreferrer" on:click={follow} on:auxclick={follow}><slot/></a>
{#if error}<span class="error" role="alert">{$_('bbcode.link_failed', { values: { error } })}</span>{/if}

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
	.error {
		display: block;
		color: var(--error);
		font-size: .85em;
	}
</style>
