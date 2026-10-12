<script>
	import Addon from "./Addon.svelte";
	import { _ } from "svelte-i18n";
	import Dead from "./Dead.svelte";
	import Loading from "./Loading.svelte";

	export let next;
	export let onClick = null;
	export let onNewAddonClick = null;

	let loading = true;
	let loaded = false;
	let error = null;
	let total = 0;
	let addons = [];
	let request = 0;

	const key = addon => addon.installed?.path ?? addon.workshop?.id;

	// A new `next` replaces the list once its first page arrives, so the current
	// cards stay on screen and cards present in both lists are kept.
	function fetchNext(reset = false) {
		const id = reset ? ++request : request;
		loading = true;

		next().then(([fetchedTotal, fetchedAddons]) => {
			if (id !== request) return;
			const base = reset ? [] : addons;
			const seen = new Set(base.map(key));
			addons = base.concat(fetchedAddons.filter(addon => !seen.has(key(addon))));
			total = fetchedTotal;
			if (reset && scroller) scroller.scrollTop = 0;
			error = null;
			loaded = true;
			loading = false;
		}, reason => {
			if (id !== request) return;
			if (reset) {
				addons = [];
				error = reason;
				loaded = true;
			}
			loading = false;
		});
	}
	$: next, fetchNext(true);

	let scroller;
	function infiniScroll() {
		if (!scroller || loading) return;
		if (scroller.offsetHeight + scroller.scrollTop >= scroller.scrollHeight * .9 && addons.length < total) {
			fetchNext();
		}
	}
</script>

<main class="hide-scroll" on:scroll={infiniScroll} bind:this={scroller}>
	{#if addons.length !== 0 && total !== 0 && addons.length !== total}
		<h2 id="amount">
			<span>{$_('showing_num_addons', { values: { num: addons.length.toLocaleString(), total: total.toLocaleString() }})}</span>
		</h2>
	{/if}

	{#if onNewAddonClick}
		<div id="grid">
			<Addon newAddon={true} onClick={onNewAddonClick}/>
			{#each addons as addon (key(addon))}
				<Addon {onClick} workshopData={addon.workshop ? Promise.resolve(addon.workshop) : null} installedData={addon.installed ? Promise.resolve(addon.installed) : null}/>
			{/each}
			{#if !loaded}
				<Loading size="2rem"/>
			{:else if error}
				<Dead size="2rem"/>
				<div id="error">{JSON.stringify(error)}</div>
			{/if}
		</div>
	{:else if !loaded}
		<Loading size="2rem"/>
	{:else if error}
		<Dead size="2rem"/>
		<div id="error">{JSON.stringify(error)}</div>
	{:else if addons.length === 0}
		<Dead size="2rem"/>
		<div id="error">{$_('ERR_NO_ADDONS_FOUND')}</div>
	{:else}
		<div id="grid">
			{#each addons as addon (key(addon))}
				<Addon {onClick} workshopData={addon.workshop ? Promise.resolve(addon.workshop) : null} installedData={addon.installed ? Promise.resolve(addon.installed) : null}/>
			{/each}
		</div>
	{/if}
</main>

<style>
	main {
		flex: 1;
		overflow: auto;
		position: relative;
		padding: 1.5rem;
		height: 100%;
	}
	main > h2 {
		margin-top: 0;
	}
	main #grid {
		position: relative;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
		grid-auto-flow: dense;
		align-items: center;
		grid-gap: 1rem;
	}
	main > :global(svg) {
		position: absolute;
		top: 1.5rem !important;
		bottom: auto !important;
	}
	main > :global(.loading) {
		opacity: .2;
	}
	#error {
		position: absolute;
		margin: auto;
		top: 4.5rem;
		text-align: center;
		padding: 1.5rem;
		left: 0;
		right: 0;
		padding-top: 0;
		padding-bottom: 0;
	}
</style>
