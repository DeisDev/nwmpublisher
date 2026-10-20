<script context="module">
	import ContextMenuItem from './ContextMenuItem.svelte';

	export const currentContext = writable(null);

	const contextRegistry = new WeakMap();

	new MutationObserver(mutations => {
		for (let x = 0; x < mutations.length; x++) {
			const mutation = mutations[x];
			if (mutation.removedNodes) {
				for (let i = 0; i < mutation.removedNodes; i++) {
					const node = mutation[i];
					if (contextRegistry.has(node)) {
						contextRegistry.delete(node);
					}
				}
			}
		}
	}).observe(document.body, { subtree: true, childList: true });

	export function registerContext(node, ...data) {
		contextRegistry.set(node, data);
	}

	function findTarget(e, target) {
		if (contextRegistry.has(target)) {

			var register = contextRegistry.get(target);
			let workshop;
			let gma;
			let disableDownload;
			let items;

			if (typeof register[0] === 'function') {
				register = register[0](e);
				if (!register) return;
			}

			[workshop, gma, disableDownload, items] = register;

			currentContext.set({
				x: e.pageX,
				y: e.pageY,
				workshop,
				gma,
				disableDownload,
				items: items ?? null
			});

			e.preventDefault();
			e.stopPropagation();

		} else if (target.parentNode) {
			return findTarget(e, target.parentNode);
		}
	}

	document.addEventListener('contextmenu', function(e) {
		if (e.target) {
			findTarget(e, e.target);
		}
	}, false);

	window.addEventListener('scroll', () => {
		const current = get(currentContext);
		if (current && current.element) {
			current.element.remove();
		}
		currentContext.set(null);
	}, true);
</script>

<script>
	import { _ } from 'svelte-i18n';
	import Folder from '@lucide/svelte/icons/folder';
	import FolderAdd from '@lucide/svelte/icons/folder-plus';
	import LinkChain from '@lucide/svelte/icons/link';
	import LinkOut from '@lucide/svelte/icons/external-link';
	import Image from '@lucide/svelte/icons/image';
	import CloudDownload from '@lucide/svelte/icons/cloud-download';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import CircleMinus from '@lucide/svelte/icons/circle-minus';
	import { addonName, addonRemoval, removalJob, requestDelete, requestUnsubscribe } from '../addon-removal.js';
	import { settings } from '../settings.js';
	import { get, writable } from 'svelte/store';
	import Loading from './Loading.svelte';
	import { onDestroy, onMount, tick } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import DestinationSelect from './DestinationSelect.svelte';
	import { formatSize } from '../format.js';
	import { taskMessage, Transaction } from '../transactions';
import { playSound } from '../sounds';

	export let workshop = null;
	export let gma = null;
	export let x;
	export let y;
	export let disableDownload = false;
	export let items = null;

	let subscription;

	const me = get(currentContext);
	if (!me) throw new Error('currentContext invalid');

	if (!items && (!workshop || !gma)) {
		throw new Error('Tried to create a context menu with no Workshop or GMA promise');
	}

	const previousFocus = document.activeElement;
	const openedByKeyboard = !!previousFocus?.matches?.(':focus-visible');

	let loading = items ? Promise.resolve() : new Promise(resolve => {
		if (workshop) {
			workshop.then(workshop => {
				resolve();
				return workshop;
			});
		}
		if (gma) {
			gma.then(gma => {
				resolve();
				return gma;
			});
		}
	});

	let destroyed = false;
	function destroy() {
		destroyed = true;

		setTimeout(() => {
			if (contextMenu) {
				contextMenu.remove();
			}
		}, 250);

		onSoftDestroy();

		if (contextMenu?.contains(document.activeElement)) {
			previousFocus?.focus?.({ preventScroll: true });
		}

		if ($currentContext == me) {
			$currentContext = null;
		}
	}

	let contextMenu;
	function updatePosition() {
		if (!contextMenu) return;

		contextMenu.style.left = Math.max(Math.min(x, window.innerWidth - contextMenu.clientWidth), 0) + 'px';

		const top = Math.max(Math.min(y, window.innerHeight - contextMenu.clientHeight), 0);
		contextMenu.style.top = top + 'px';
		contextMenu.style.transformOrigin = y >= top ? 'top' : 'bottom';
	}

	onMount(async () => {
		me.element = contextMenu;
		new ResizeObserver(updatePosition).observe(contextMenu);
		updatePosition();

		await loading;
		await tick();
		if (destroyed) return;
		const first = openedByKeyboard && contextMenu.querySelector('[role="menuitem"]:not([aria-disabled="true"])');
		(first || contextMenu).focus({ preventScroll: true });
	});

	function onKeydown(event) {
		const items = [...contextMenu.querySelectorAll('[role="menuitem"]:not([aria-disabled="true"])')];
		const index = items.indexOf(document.activeElement);
		let next;
		switch (event.key) {
			case 'ArrowDown':
				next = items[(index + 1) % items.length];
				break;
			case 'ArrowUp':
				next = items[index <= 0 ? items.length - 1 : index - 1];
				break;
			case 'Home':
				next = items[0];
				break;
			case 'End':
				next = items[items.length - 1];
				break;
			case 'Escape':
			case 'Tab':
				event.preventDefault();
				destroy();
				return;
			default:
				return;
		}
		event.preventDefault();
		next?.focus({ preventScroll: true });
	}
	$: { updatePosition(); }

	function onClickOff(e) {
		if (!contextMenu) return;
		destroy();
	}
	onMount(() => {
		document.addEventListener('click', onClickOff);
	});

	function onSoftDestroy() {
		document.removeEventListener('click', onClickOff);

		if (subscription) {
			subscription();
			subscription = null;
		}
	}
	onDestroy(onSoftDestroy);

	subscription = currentContext.subscribe(val => {
		if (val == me) return;
		destroy();
	});

	async function openAddonLocation() {
		invoke('open_file_location', { path: (await gma).path });
	}

	async function copyLink() {
		navigator.clipboard.writeText('https://steamcommunity.com/sharedfiles/filedetails/?id=' + (await workshop).id);
	}

	async function copyImageLink() {
		navigator.clipboard.writeText((await workshop).previewUrl);
	}

	let choosingExtractDestination = false;
	let extractedName;
	let gmaSize;
	async function chooseExtractDestination() {
		let awaitedGma = await gma;
		gmaSize = awaitedGma.size;
		extractedName = awaitedGma.extractedName;
		choosingExtractDestination = true;
	}
	async function extractGMA(dest) {
		choosingExtractDestination = false;

		invoke('extract_gma', { gmaPath: (await gma).path, dest })
			.then(transactionId => new Transaction(transactionId, transaction => {
				if (transaction.status === 'verifying') return $_('verifying', { values: { progress: transaction.progress + '%' } });
				return $_('extracting_progress', { values: {
					pct: transaction.progress,
					data: formatSize((transaction.progress / 100) * gmaSize),
					dataTotal: formatSize(gmaSize)
				}});
			}));
	}

	let workshopItem = null;
	workshop?.then(item => workshopItem = item, () => {});

	async function download() {
		await invoke('workshop_download', { ids: [(await workshop).id] });

		playSound('success');
		taskMessage($_('sent_to_downloader'));
	}
</script>

<DestinationSelect active={choosingExtractDestination} cancel={() => choosingExtractDestination = false} callback={extractGMA} text={$_('extract')} extractedName={extractedName}/>

<div class="context-menu" role="menu" tabindex="-1" bind:this={contextMenu} class:destroyed={destroyed} on:resize={updatePosition} on:keydown={onKeydown}>
	{#await loading}
		<Loading size="2rem"/>
	{:then}
		{#if items}
			{#each items as item}
				{#if item.divider}
					<div class="divider" role="separator"></div>
				{:else}
					<ContextMenuItem click={item.click} disabled={item.disabled}>
						<span slot="icon"><svelte:component this={item.icon} class="icon" size="1rem"/></span>
						<span slot="label">{item.label}</span>
					</ContextMenuItem>
				{/if}
			{/each}
		{:else if gma}
			{#await gma then gma}
				{#if gma}
					{#if gma.path}
						<ContextMenuItem click={chooseExtractDestination}>
							<span slot="icon"><FolderAdd class="icon" size="1rem"/></span>
							<span slot="label">{$_('extract')}</span>
						</ContextMenuItem>
						<ContextMenuItem click={openAddonLocation}>
							<span slot="icon"><Folder class="icon" size="1rem"/></span>
							<span slot="label">{$_('open_addon_location')}</span>
						</ContextMenuItem>
						{#await addonRemoval(gma.path) then removal}
							{#if removal === 'unsubscribe'}
								<ContextMenuItem click={() => requestUnsubscribe(gma.path, addonName(workshopItem, gma))} disabled={!!$removalJob}>
									<span slot="icon"><CircleMinus class="icon" size="1rem"/></span>
									<span slot="label">{$_($settings.unsubscribe_delete_files ? 'addon_unsubscribe_delete' : 'addon_unsubscribe')}</span>
								</ContextMenuItem>
							{:else if removal === 'delete'}
								<ContextMenuItem click={() => requestDelete(gma.path, addonName(workshopItem, gma))} disabled={!!$removalJob}>
									<span slot="icon"><Trash2 class="icon" size="1rem"/></span>
									<span slot="label">{$_('addon_delete')}</span>
								</ContextMenuItem>
							{/if}
						{/await}
					{/if}
				{/if}
			{/await}
		{/if}
		{#if !items}
			{#await Promise.allSettled([workshop, gma]) then [workshop, gma]}
				{#if workshop?.value && gma?.value}
					<div class="divider" role="separator"></div>
				{/if}
			{/await}
		{/if}
		{#if workshop && !items}
			{#await workshop then workshop}
				{#if workshop && !workshop.dead}
					<a class="nostyle" href="https://steamcommunity.com/sharedfiles/filedetails/?id={workshop.id}" target="_blank" tabindex="-1">
						<ContextMenuItem>
							<span slot="icon"><LinkOut class="icon" size="1rem"/></span>
							<span slot="label">{$_('steam_workshop')}</span>
						</ContextMenuItem>
					</a>
					<ContextMenuItem click={copyLink}>
						<span slot="icon"><LinkChain class="icon" size="1rem"/></span>
						<span slot="label">{$_('copy_link')}</span>
					</ContextMenuItem>
					{#if !disableDownload}
						<ContextMenuItem click={download}>
							<span slot="icon"><CloudDownload class="icon" size="1rem"/></span>
							<span slot="label">{$_('download')}</span>
						</ContextMenuItem>
					{/if}
					{#if workshop.previewUrl}
						<div class="divider" role="separator"></div>
						<a class="nostyle" href={workshop.previewUrl} target="_blank" tabindex="-1">
							<ContextMenuItem>
								<span slot="icon"><LinkOut class="icon" size="1rem"/></span>
								<span slot="label">{$_('open_image')}</span>
							</ContextMenuItem>
						</a>
						<ContextMenuItem click={copyImageLink}>
							<span slot="icon"><Image class="icon" size="1rem"/></span>
							<span slot="label">{$_('copy_image_link')}</span>
						</ContextMenuItem>
					{/if}
				{/if}
			{/await}
		{/if}
	{/await}
</div>

<style>
	.context-menu {
		position: absolute;
		animation: context-menu .25s forwards;
		z-index: 9999;
		background-color: var(--bg-menu);
		color: var(--text);
		font-size: .9em;
		border-radius: .2rem;
		overflow: hidden;
		width: max-content;
		transform-origin: top;
	}
	.context-menu:focus {
		outline: none;
	}
	.context-menu.destroyed {
		animation: context-menu-reverse .25s forwards;
		pointer-events: none;
		-webkit-pointer-events: none;
	}

	.divider {
		background-color: var(--bg-menu-hover);
		height: 1px;
	}

	@keyframes context-menu {
		from {
			opacity: 0;
			transform: scaleY(0);
		}
		to {
			opacity: 1;
			transform: scaleY(1);
		}
	}
	@keyframes context-menu-reverse {
		from {
			opacity: 1;
			transform: scaleY(1);
		}
		to {
			opacity: 0;
			transform: scaleY(0);
		}
	}
</style>
