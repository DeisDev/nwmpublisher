<script>
	import { onMount } from "svelte";
	import { open } from '@tauri-apps/plugin-shell';
	import Modal from './Modal.svelte';
	import { _ } from 'svelte-i18n';
	import Logo from "./Logo.svelte";
	export let active = false;
	onMount(() => active = true);

	const links = [
		{ key: 'github_star_plz_i_need_a_job_maybe', href: 'https://github.com/DeisDev/nwmpublisher' },
		{ key: 'vscode_glua_enhanced', href: 'https://marketplace.visualstudio.com/items?itemName=venner.vscode-glua-enhanced' },
	];

	async function openLink(event) {
		const url = event.currentTarget.href;
		try {
			await open(url);
		} catch (error) {
			console.error('Failed to open link:', url, error);
		}
	}

	function dismiss() {
		active = false;
	}
</script>

<Modal id="github-star-modal" {active} cancel={dismiss}>
	<Logo/>
	<h2>{$_('enjoying_nwmpublisher')}<img src="/img/dog.gif"/></h2>
	<p>
		{#each links as link, index}
			{#if index > 0}<br>{/if}
			<span>{#each $_(link.key).split(/%(.+?)%/g) as part, partIndex}{#if partIndex % 2}<a class="color" href={link.href} on:click|preventDefault={openLink}>{part}</a>{:else}{part}{/if}{/each}</span>
		{/each}
	</p>
	<button class="btn" type="button" on:click={dismiss}>{$_('done')}</button>
</Modal>

<style>
	:global(#github-star-modal > .hide-scroll) {
		text-align: center;
		padding: 1.5rem;
		width: 31rem;
		height: 31rem;
	}
	:global(#github-star-modal p) {
		line-height: 2.4;
	}

	h2 img {
		margin-left: .5rem;
	}
	h2 {
		display: flex;
		justify-content: center;
		align-items: center;
		margin-top: 0;
	}

	:global(#github-star-modal #logo) {
		transform: rotate(-10deg);
		margin-bottom: 1.5rem;
		margin-top: 1rem;
		width: 10rem;
		height: auto;
	}
	.btn {
		width: 100%;
		border: 0;
		color: inherit;
		font: inherit;
		cursor: pointer;
		background: #313131;
		box-shadow: 0px 0px 2px 0px rgb(0 0 0 / 40%);
		border-radius: 4px;
		padding: .7rem;
		margin-top: 1rem;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.btn:active {
		background: #252525;
	}
</style>
