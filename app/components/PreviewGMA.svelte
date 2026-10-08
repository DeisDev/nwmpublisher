<script>
	import { Steam } from '../steam.js';
	import { _, locale } from 'svelte-i18n';
	import { translateError } from '../i18n';
	import { formatSize } from '../format.js';
	import Dead from './Dead.svelte';
	import SteamID from 'steamid';
	import LinkOut from '@lucide/svelte/icons/external-link';
	import { invoke } from '@tauri-apps/api/core';
	import WorkshopStats from './WorkshopStats.svelte';
	import BBCode from './BBCode.svelte';
	import { onDestroy } from 'svelte';
	import { Transaction } from '../transactions.js';
	import Loading from './Loading.svelte';
	import Modal from './Modal.svelte';
	import Addon from './Addon.svelte';
	import FileBrowser from './FileBrowser.svelte';
	import FilePreview from './FilePreview.svelte';
	import DestinationSelect from './DestinationSelect.svelte';
	import { writable } from 'svelte/store';

	export let active = false;
	export let promises;
	export let cancel;

	let activeTab = 'details';
	const tabs = ['details', 'files'];
	function onTabKeydown(event, index) {
		let next;
		if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') next = 1 - index;
		else if (event.key === 'Home') next = 0;
		else if (event.key === 'End') next = 1;
		else return;
		event.preventDefault();
		activeTab = tabs[next];
		event.currentTarget.parentElement.children[next].focus();
	}

	let selection = 0;
	onDestroy(() => selection++);

	let gmaSize;
	let gmaPath;
	let selectedEntry = null;
	let entriesList = writable([]);

	function extractEntry(entryPath) {
		if (!gmaPath) return;
		const size = gmaSize;
		invoke('extract_preview_entry', { gmaPath, entryPath })
			.then(transactionId => new Transaction(transactionId, transaction => {
				return $_('extracting_progress', { values: {
					pct: transaction.progress,
					data: formatSize((transaction.progress / 100) * size),
					dataTotal: formatSize(size)
				}});
			}));
	}

	function extractGMA(dest) {
		destinationSelect = false;

		if (!gmaPath) return;
		const size = gmaSize;
		invoke('extract_preview_gma', { gmaPath, dest })
			.then(transactionId => new Transaction(transactionId, transaction => {
				return $_('extracting_progress', { values: {
					pct: transaction.progress,
					data: formatSize((transaction.progress / 100) * size),
					dataTotal: formatSize(size)
				}});
			}));
	}

	let destinationSelect = false;
	function chooseDestination() {
		if (!gmaPath) return;
		destinationSelect = true;
	}

	let addon = new Promise(() => {});
	let previewError = null;
	function updatePromises(promises) {
		const currentSelection = ++selection;
		activeTab = 'details';
		selectedEntry = null;
		const [workshop, gma] = promises;
		addon = new Promise(() => {});
		gmaPath = null;
		gmaSize = 0;
		$entriesList = [];
		previewError = null;
		destinationSelect = false;

		let workshopData = null;
		let gmaData = null;
		let previewPath = null;

		async function update() {
			if (selection !== currentSelection) return;
			addon = Promise.resolve([workshopData, gmaData]);
			gmaSize = gmaData?.size ?? workshopData?.fileSize ?? 0;
			const path = gmaData?.path ?? workshopData?.localFile ?? null;
			if (path === previewPath) return;
			previewPath = path;
			selectedEntry = null;
			gmaPath = null;
			$entriesList = [];
			previewError = null;
			if (!path) return;

			try {
				const entries = await invoke('preview_gma', { path });
				if (selection !== currentSelection || previewPath !== path) return;
				$entriesList = entries;
				gmaPath = path;
			} catch (error) {
				if (selection !== currentSelection || previewPath !== path) return;
				previewError = translateError(error);
			}
		}

		Promise.resolve(workshop).then(data => {
			workshopData = data;
			return update();
		}, () => update()); // Local archives can have no available Workshop item.

		Promise.resolve(gma).then(data => {
			gmaData = data;
			return update();
		}, error => {
			if (selection !== currentSelection) return;
			previewError = translateError(error);
			addon = Promise.resolve([workshopData, gmaData]);
		});
	}
	onDestroy(promises.subscribe(updatePromises));

	function open() {
		invoke('open_file_location', { path: gmaPath });
	}

	async function interceptCancel() {
		selection++;
		gmaPath = null;
		await invoke('preview_gma', { path: null });
		destinationSelect = false;
		cancel();
	}
</script>

<Modal id="gma-preview" {active} cancel={interceptCancel}>
	{#await addon}
		<Loading size="2rem"/>
	{:then [workshop, gma]}
		{#if !workshop && !gma && previewError}
			<p class="select">{$_('addon_preview_error', { values: { error: previewError } })}</p>
		{:else if !workshop && !gma}
			<Dead size="2rem"/>
		{:else}
			<div id="content">
				<div class="preview-toolbar">
					<div class="preview-tabs" role="tablist" aria-label={$_('addon_preview.tabs')}>
						{#each tabs as tab, index}
							<button type="button" role="tab" id={'preview-tab-' + tab} aria-controls={'preview-panel-' + tab} aria-selected={activeTab === tab} tabindex={activeTab === tab ? 0 : -1} on:click={() => activeTab = tab} on:keydown={event => onTabKeydown(event, index)}>{$_('addon_preview.' + tab)}</button>
						{/each}
					</div>
					<div class="preview-actions">
						{#if (gma && gma.id) || workshop}
							<div id="ws-link"><a class="color" href="https://steamcommunity.com/sharedfiles/filedetails/?id={gma?.id ?? workshop.id}" target="_blank">{$_('steam_workshop')}<LinkOut class="icon" size=".8rem"/></a></div>
						{/if}
						<button type="button" class="extract-btn" disabled={!gmaPath} on:click={chooseDestination}>{$_('extract')}</button>
					</div>
				</div>
				<div id="preview-panel-details" class="preview-panel details-panel" role="tabpanel" aria-labelledby="preview-tab-details" tabindex="0" hidden={activeTab !== 'details'}>
					<div id="addon">
						{#key $promises}
						<Addon previewing={true} workshopData={$promises[0]} installedData={$promises[1]} fallbackName={gma ? (gma.name ?? gma.extracted_name) : null}>
						<div class="addon-metadata">
						{#if workshop}
							<div id="tags">
								{#if workshop.tags}
									{#if gma && gma.type && workshop.tags.indexOf(gma.type.toLowerCase()) !== -1}
										<div class="tag {gma.type.toLowerCase()}">{gma.type}</div>
									{/if}
									{#each workshop.tags as tag}
										<div class="tag {tag.toLowerCase()}">{tag}</div>
									{/each}
								{/if}
								{#if gma && gma.tags}
									{#each gma.tags as tag}
										{#if !workshop.tags || workshop.tags.indexOf(tag) !== -1}
											<div class="tag {tag.toLowerCase()}">{tag}</div>
										{/if}
									{/each}
								{/if}
							</div>
						{:else if gma}
							{#if gma.tags}
								<div id="tags">
									{#if gma.type && gma.tags.indexOf(gma.type.toLowerCase()) !== -1}
										<div class="tag {gma.type.toLowerCase()}">{gma.type}</div>
									{/if}
									{#each gma.tags as tag}
										<div class="tag {tag.toLowerCase()}">{tag}</div>
									{/each}
								</div>
							{/if}
						{/if}
						<table id="addon-info">
							<tbody>
								{#if gma && gma.size > 0}
									<tr>
										<th>{$_('size')}</th>
										<td>{formatSize(gma.size, $locale)}</td>
									</tr>
								{:else if workshop && workshop.fileSize > 0}
									<tr>
										<th>{$_('size')}</th>
										<td>{formatSize(workshop.fileSize, $locale)}</td>
									</tr>
								{/if}
								{#if workshop}
									{#if workshop.owner}
										<tr>
											<th>{$_('author')}</th>
											<td>
												<a target="_blank" class="nostyle" href="https://steamcommunity.com/profiles/{workshop.owner.steamid64}">
													<img id="avatar" src="data:image/png;base64,{workshop.owner.avatar}"/>
													<span>{workshop.owner.name}</span>
												</a>
											</td>
										</tr>
									{:else if workshop.steamid64}
										<tr>
											<th>{$_('author')}</th>
											<td>
												{#await Steam.getSteamUser(workshop.steamid64)}
													<div id="author-loading">
														<a target="_blank" class="nostyle" href="https://steamcommunity.com/profiles/{workshop.steamid64}">
															<img id="avatar" src="/img/steam_anonymous.jpg"/>
															{new SteamID(workshop.steamid64).getSteam2RenderedID(true)}
														</a>
													</div>
												{:then owner}
													<a target="_blank" class="nostyle" href="https://steamcommunity.com/profiles/{workshop.steamid64}">
														<img id="avatar" src="data:image/png;base64,{owner.avatar}"/>
														<span>{owner.name}</span>
													</a>
												{:catch}
													<div id="author-loading">
														<a target="_blank" class="nostyle" href="https://steamcommunity.com/profiles/{workshop.steamid64}">
															<img id="avatar" src="/img/steam_anonymous.jpg"/>
															{new SteamID(workshop.steamid64).getSteam2RenderedID(true)}
														</a>
													</div>
													&nbsp;<Dead inline="true"/>
												{/await}
											</td>
										</tr>
									{/if}
								{/if}
							</tbody>
						</table>
						</div>
						</Addon>
						{/key}
					</div>
					{#if workshop}<WorkshopStats item={workshop}/>{/if}
					<div id="description" class="select">
						{#if workshop?.description ?? gma?.description}
							<BBCode value={workshop?.description ?? gma.description}/>
						{:else}
							<p class="empty-description">{$_('addon_preview.no_description')}</p>
						{/if}
					</div>
				</div>
				<div id="preview-panel-files" class="preview-panel files-panel" role="tabpanel" aria-labelledby="preview-tab-files" tabindex="0" hidden={activeTab !== 'files'}>
				{#if previewError}
					<p class="select">{$_('addon_preview_error', { values: { error: previewError } })}</p>
				{:else if gmaPath}
					<div class="file-layout" class:has-preview={selectedEntry !== null}>
						<div class="file-list"><FileBrowser browsePath={gmaPath} {entriesList} {open} openEntry={extractEntry} previewEntry={path => selectedEntry = path} selectedPath={selectedEntry}/></div>
						{#if active && activeTab === 'files' && selectedEntry}
							<FilePreview {gmaPath} entryPath={selectedEntry} close={() => selectedEntry = null} openEntry={extractEntry}/>
						{/if}
					</div>
				{:else if gma || workshop?.localFile}
					<Loading size="2rem"/>
				{:else}
					<Dead size="2rem"/>
				{/if}
				</div>
			</div>

			<DestinationSelect active={destinationSelect} cancel={() => destinationSelect = false} callback={extractGMA} text={$_('extract')} extractedName={gma?.extractedName} />
		{/if}
	{/await}
</Modal>

<style>
	:global(#gma-preview > .hide-scroll) {
		max-width: 100%;
   		max-height: 100%;
		width: 76rem;
		height: 50rem;
	}
	@media (max-width: 76rem), (max-height: 50rem) {
		:global(#gma-preview > .hide-scroll) {
			width: 100%;
			height: 100%;
		}
	}
	:global(#gma-preview) #content {
		display: flex;
		flex-direction: column;
		min-width: 0;
		background-color: var(--bg-content);
		height: 100%;
		box-shadow: rgba(0, 0, 0, .24) 0px 3px 8px;
	}
	:global(#gma-preview > div > .dead), :global(#gma-preview > div > .loading) {
		position: absolute;
	}
	:global(#gma-preview > div) {
		border-top-left-radius: 0 !important;
	}

	.preview-toolbar, .preview-tabs, .preview-actions {
		display: flex;
		align-items: center;
		gap: .5rem;
	}
	.preview-toolbar {
		justify-content: space-between;
		padding-inline: 1rem;
		border-bottom: 1px solid var(--border);
		background: var(--bg-panel);
		flex-shrink: 0;
	}
	.preview-tabs button {
		padding: .9rem 1.25rem;
		border: 0;
		border-bottom: 2px solid transparent;
		background: transparent;
		font: inherit;
		color: var(--text-muted);
		cursor: pointer;
	}
	.preview-tabs button[aria-selected='true'] {
		border-bottom-color: var(--text);
		color: var(--text);
		background: var(--bg-raised);
	}
	.preview-tabs button:hover { background: var(--control); }
	button:focus-visible, .preview-panel:focus-visible {
		outline: 2px solid #127cff;
		outline-offset: -2px;
	}
	.preview-panel {
		flex: 1;
		min-height: 0;
		min-width: 0;
		overflow: auto;
	}
	.preview-panel[hidden] { display: none; }
	.details-panel { padding: 1.5rem 2rem; --workshop-stats-columns: repeat(4, minmax(0, 1fr)); }
	.preview-actions { gap: 1rem; }
	.files-panel { display: flex; }
	.file-layout { display: flex; flex: 1; min-width: 0; min-height: 0; }
	.file-list { display: flex; flex: 1; min-width: 0; min-height: 0; }
	.has-preview .file-list { flex: 0 0 42%; }
	@media (max-width: 900px) {
		.file-layout { flex-direction: column; }
		.has-preview .file-list { flex: 0 0 38%; min-height: 8rem; }
	}
	#addon { margin-bottom: 1.5rem; }
	.addon-metadata { display: flex; flex-direction: column; gap: .6rem; min-width: 0; }
	#tags { display: flex; flex-wrap: wrap; gap: .35rem .6rem; line-height: 1.4; }
	#tags :global(.tag) { margin: 0; }
	#addon-info { order: -1; border-collapse: collapse; text-align: start; font-size: .85rem; }
	#addon-info tbody { display: flex; flex-wrap: wrap; gap: .5rem 1.5rem; }
	#addon-info tr { display: flex; align-items: center; gap: .5rem; }
	#addon-info th { color: var(--text-muted); font-weight: 400; }
	#addon-info th, #addon-info td { padding: 0; }
	#description {
		margin-top: 1.5rem;
		color: var(--text);
		line-height: 1.6;
	}
	.empty-description { color: var(--text-muted); }
	@media (max-width: 600px) {
		.details-panel { padding: 1rem; --workshop-stats-columns: repeat(2, minmax(0, 1fr)); }
		.preview-toolbar { padding-inline: .5rem; }
		.preview-tabs button { padding-inline: .75rem; }
		.preview-actions { gap: .5rem; }
	}

	#addon #avatar, #addon #avatar + span {
		vertical-align: middle;
		width: 1.5rem;
		border-radius: 50%;
	}
	#addon #avatar {
		margin-right: .2rem;
	}

	#ws-link { font-size: .85rem; }
	#ws-link a { display: inline-flex; align-items: center; gap: .3rem; }

	.extract-btn {
		font: inherit;
		border: 0;
		border-radius: 4px;
		padding: .7rem;
		text-align: center;
		background-color: var(--neutral);
		color: #fff;
		z-index: 3;
		box-shadow: 0 0 5px rgba(0, 0, 0, .1);
		cursor: pointer;
		text-shadow: 0px 1px 0px rgba(0, 0, 0, .6);
		line-height: 1;
		transition: background-color .5s;
	}
	.extract-btn:disabled {
		cursor: default;
		color: var(--text-muted);
		background-color: var(--control-hover-alt);
	}

	#author-loading {
		display: flex;
		width: 100%;
	}
	#author-loading > a {
		flex: 1;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
</style>
