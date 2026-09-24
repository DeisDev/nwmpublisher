<script>
	import { _, locale } from 'svelte-i18n';
	import { invoke, Channel } from '@tauri-apps/api/core';
	import { onDestroy } from 'svelte';
	import ScanSearch from '@lucide/svelte/icons/scan-search';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import Package from '@lucide/svelte/icons/package';
	import Loading from '../components/Loading.svelte';
	import Modal from '../components/Modal.svelte';
	import { formatSize } from '../format.js';
	import { saveSettings } from '../settings.js';

	let scan = null;
	let cleaner;
	let selected = [];
	let busy = false;
	let saving = false;
	let confirming = false;
	let error = null;
	let result = null;
	let permanent = AppSettings.addon_cleaner_permanent_delete ?? false;
	const pageSize = 50;
	let page = 0;
	let details = {};
	let failedImages = {};
	let detailsLoading = false;
	let detailsError = null;
	let detailsCount = 0;
	let detailsStarted = 0;
	let detailsRequest = 0;
	let operation = 0;
	let progress = null;
	let started = 0;
	let lastUpdate = 0;
	let now = Date.now();
	const clock = setInterval(() => { if (busy || detailsLoading) now = Date.now(); }, 1000);
	onDestroy(() => { clearInterval(clock); operation++; detailsRequest++; });
	$: visibleFiles = scan?.files.slice(page * pageSize, (page + 1) * pageSize) ?? [];
	$: selectedSet = new Set(selected);
	$: selectedSize = selected.reduce((size, index) => size + (scan?.files[index]?.size ?? 0), 0);
	$: elapsed = Math.max(0, Math.floor((now - started) / 1000));
	$: quietSeconds = Math.max(0, Math.floor((now - lastUpdate) / 1000));
	$: detailsSeconds = Math.max(0, Math.floor((now - detailsStarted) / 1000));

	function startProgress() {
		cleaner?.scrollTo({ top: 0 });
		const current = ++operation;
		started = lastUpdate = now = Date.now();
		progress = { stage: 'subscriptions', completed: 0, total: null, found: 0 };
		return new Channel(update => {
			if (current !== operation || !busy) return;
			progress = update;
			lastUpdate = now = Date.now();
		});
	}

	function toggleFile(index, checked) {
		selected = checked ? [...selected, index] : selected.filter(value => value !== index);
	}

	async function loadDetails() {
		const scanId = scan?.scanId;
		if (!scanId) return;
		const current = ++detailsRequest;
		const seen = new Set();
		const indices = scan.files.slice(page * pageSize, (page + 1) * pageSize).flatMap((file, index) => {
			if (details[file.workshopId] || seen.has(file.workshopId)) return [];
			seen.add(file.workshopId);
			return [page * pageSize + index];
		});
		detailsError = null;
		if (!indices.length) { detailsLoading = false; return; }
		detailsCount = indices.length;
		detailsStarted = now = Date.now();
		detailsLoading = true;
		try {
			const items = await invoke('addon_cleaner_details', { scanId, selected: indices });
			if (current !== detailsRequest || scan?.scanId !== scanId) return;
			details = { ...details, ...Object.fromEntries(items.map(item => [item.id, item])) };
		} catch (err) {
			if (current === detailsRequest) detailsError = err;
		} finally {
			if (current === detailsRequest) detailsLoading = false;
		}
	}

	async function changePage(next) {
		page = next;
		cleaner?.scrollTo({ top: 0 });
		await loadDetails();
	}

	function errorText(value) {
		return value?.key ? $_(value.key, { values: { detail: value.detail } }) : String(value);
	}

	async function scanAddons() {
		busy = true;
		error = null;
		result = null;
		scan = null;
		selected = [];
		page = 0;
		details = {};
		failedImages = {};
		detailsError = null;
		detailsLoading = false;
		detailsRequest++;
		const onProgress = startProgress();
		try {
			scan = await invoke('scan_addon_cleaner', { onProgress });
			selected = scan.files.map((_, index) => index);
		} catch (err) {
			error = err;
		} finally {
			busy = false;
			operation++;
		}
		if (scan) loadDetails();
	}

	async function changeMode(event) {
		const control = event.currentTarget;
		const next = control.value === 'permanent';
		saving = true;
		error = null;
		try {
			await saveSettings({ addon_cleaner_permanent_delete: next });
			permanent = next;
		} catch (err) {
			error = err;
		} finally {
			saving = false;
			control.value = permanent ? 'permanent' : 'trash';
		}
	}

	async function clean() {
		confirming = false;
		busy = true;
		error = null;
		const onProgress = startProgress();
		try {
			result = await invoke('clean_addons', { scanId: scan.scanId, selected, permanent, onProgress });
			scan = null;
			selected = [];
			detailsRequest++;
			detailsLoading = false;
			detailsError = null;
		} catch (err) {
			error = err;
		} finally {
			busy = false;
			operation++;
		}
	}
</script>

<section class="cleaner" bind:this={cleaner}>
	<header>
		<h2>{$_('addon_cleaner')}</h2>
		<p>{$_('cleaner_intro')}</p>
		<div class="toolbar">
			<button class="scan" on:click={scanAddons} disabled={busy || saving}><ScanSearch size={18}/>{$_('cleaner_scan')}</button>
			<label class="mode">
				<span>{$_('cleaner_removal')}</span>
				<select value={permanent ? 'permanent' : 'trash'} on:change={changeMode} disabled={busy || saving}>
					<option value="trash">{$_('cleaner_trash')}</option>
					<option value="permanent">{$_('cleaner_permanent')}</option>
				</select>
			</label>
		</div>
	</header>
	{#if busy && progress}
		<div class="progress-panel">
			<div class="progress" role="status"><Loading size="1.5rem"/>
				<span>{$_(progress.stage === 'subscriptions' && progress.total === null ? 'cleaner_progress_connecting' : `cleaner_progress_${progress.stage}`, { values: { completed: progress.completed, found: progress.found, total: progress.total ?? 0 } })}</span>
			</div>
			{#if progress.total !== null && progress.total > 0}
				<progress value={progress.completed} max={progress.total} aria-label={$_(`cleaner_stage_${progress.stage}`)}></progress>
			{/if}
			{#if progress.source}<small>{$_(`cleaner_source_${progress.source}`)}</small>{/if}
			{#if progress.path}<small class="path">{progress.path}</small>{/if}
			<small>{$_('cleaner_elapsed', { values: { seconds: elapsed } })}
				{#if quietSeconds >= 2} · {$_(progress.stage === 'subscriptions' ? 'cleaner_waiting_steam' : 'cleaner_last_update', { values: { seconds: quietSeconds } })}{/if}
			</small>
		</div>
	{/if}
	{#if error}<p class="error" role="alert">{errorText(error)}</p>{/if}
	{#if result}
		<div class="result" role="status">
			<p>{$_('cleaner_result', { values: { removed: result.removed.length, failed: result.failed.length } })}</p>
			{#each result.failed as failure}
				<p class="error"><span class="path">{failure.path}</span><br>{errorText(failure.error)}</p>
			{/each}
		</div>
	{/if}
	{#if scan}
		<p class="account">{$_('cleaner_account', { values: { account: scan.account } })}</p>
		{#if scan.files.length === 0}
			<p role="status">{$_('cleaner_empty')}</p>
		{:else}
			{#if detailsLoading}
				<p class="details-status" role="status">{$_('cleaner_details_loading', { values: { count: detailsCount, seconds: detailsSeconds } })}</p>
			{/if}
			{#if detailsError}
				<p class="error" role="alert">{errorText(detailsError)}</p>
				<button disabled={busy} on:click={loadDetails}>{$_('cleaner_retry_details')}</button>
			{/if}
			<div class="selection">
				<label><input type="checkbox" checked={selected.length === scan.files.length} disabled={busy} on:change={event => selected = event.currentTarget.checked ? scan.files.map((_, index) => index) : []}>{$_('cleaner_select_all', { values: { count: scan.files.length } })}</label>
				<span>{$_('cleaner_selected', { values: { count: selected.length, size: formatSize(selectedSize, $locale) } })}</span>
			</div>
			<div class="files">
				{#each visibleFiles as file, index (file.path)}
					<div class="file">
						<input type="checkbox" checked={selectedSet.has(page * pageSize + index)} on:change={event => toggleFile(page * pageSize + index, event.currentTarget.checked)} disabled={busy}
							aria-label={$_('cleaner_select_file', { values: { name: details[file.workshopId]?.title ?? file.fileName } })}>
						<div class="preview">
							{#if details[file.workshopId]?.previewUrl && !failedImages[file.workshopId]}
								<img src={details[file.workshopId].previewUrl} width="64" height="64" alt="" loading="lazy" decoding="async" on:error={() => failedImages = { ...failedImages, [file.workshopId]: true }}>
							{:else}<Package size={28}/>{/if}
						</div>
						<div class="file-info">
							<a class="title" href={`https://steamcommunity.com/sharedfiles/filedetails/?id=${file.workshopId}`} target="_blank" rel="noreferrer">{details[file.workshopId]?.title ?? file.fileName}</a>
							{#if details[file.workshopId] && !details[file.workshopId].title}<small>{$_('cleaner_details_unavailable')}</small>{/if}
							<small>{$_(`cleaner_source_${file.source}`)} · {$_('cleaner_workshop_id', { values: { id: file.workshopId } })}</small>
							<small class="path">{file.path}</small>
						</div>
						<span class="size">{formatSize(file.size, $locale)}</span>
					</div>
				{/each}
			</div>
			{#if scan.files.length > pageSize}
				<nav class="pagination" aria-label={$_('cleaner_pages')}>
					<button disabled={busy || detailsLoading || page === 0} on:click={() => changePage(page - 1)}>{$_('cleaner_previous')}</button>
					<span>{$_('cleaner_page', { values: { page: page + 1, pages: Math.ceil(scan.files.length / pageSize) } })}</span>
					<button disabled={busy || detailsLoading || (page + 1) * pageSize >= scan.files.length} on:click={() => changePage(page + 1)}>{$_('cleaner_next')}</button>
				</nav>
			{/if}
			<footer>
				<button class="remove" disabled={busy || saving || selected.length === 0} on:click={() => confirming = true}><Trash2 size={18}/>{$_(permanent ? 'cleaner_delete_selected' : 'cleaner_trash_selected')}</button>
			</footer>
		{/if}
	{/if}
</section>

{#if confirming}
	<Modal active={true} cancel={() => confirming = false} padding="1.5rem">
		<div class="confirmation" role="dialog" aria-modal="true" aria-labelledby="cleaner-confirm-title">
			<h2 id="cleaner-confirm-title">{$_(permanent ? 'cleaner_permanent' : 'cleaner_trash')}</h2>
			<p>{$_(permanent ? 'cleaner_confirm_delete' : 'cleaner_confirm_trash', { values: { count: selected.length, size: formatSize(selectedSize, $locale) } })}</p>
			<div class="actions">
				<button on:click={() => confirming = false}>{$_('cancel')}</button>
				<button class="remove" on:click={clean}>{$_(permanent ? 'cleaner_permanent' : 'cleaner_trash')}</button>
			</div>
		</div>
	</Modal>
{/if}

<style>
	.cleaner { height: 100%; overflow: auto; padding: 1.5rem; }
	h2 { margin: 0 0 .75rem; }
	p { line-height: 1.5; }
	header > p, .account, small { color: #aaa; }
	.toolbar, .selection, .actions, footer { display: flex; align-items: center; flex-wrap: wrap; gap: .75rem; }
	.toolbar { margin: 1rem 0; }
	.mode { display: flex; align-items: center; gap: .5rem; flex-wrap: wrap; }
	button, select { padding: .65rem .85rem; border: 0; border-radius: .25rem; color: #fff; background: #313131; font: inherit; }
	button { display: inline-flex; align-items: center; justify-content: center; gap: .5rem; cursor: pointer; }
	button.scan { background: var(--neutral); }
	button.remove { background: var(--error); }
	button:hover:not(:disabled) { filter: brightness(1.15); }
	button:disabled, select:disabled { opacity: .45; cursor: default; }
	button:focus-visible, select:focus-visible, input:focus-visible { outline: 2px solid #fff; outline-offset: 2px; }
	.selection { justify-content: space-between; margin: 1rem 0; }
	.selection label { display: flex; align-items: center; gap: .5rem; }
	.files { background: #131313; border-radius: .3rem; }
	.file { display: flex; align-items: center; gap: .75rem; padding: .85rem; }
	.file + .file { border-top: 1px solid #313131; }
	.file:hover { background: #212121; }
	.file-info { flex: 1; min-width: 0; }
	.title { color: #fff; font-weight: 600; overflow-wrap: anywhere; }
	.title:hover { color: #46b0ff; }
	.preview { display: flex; align-items: center; justify-content: center; width: 64px; height: 64px; flex-shrink: 0; background: #212121; color: #aaa; }
	.preview img { width: 100%; height: 100%; object-fit: contain; }
	.path { overflow-wrap: anywhere; }
	small { display: block; margin-top: .3rem; }
	.size { white-space: nowrap; }
	input[type='checkbox'] { flex-shrink: 0; accent-color: var(--neutral); }
	footer { justify-content: flex-end; margin-top: 1rem; }
	.error { color: #fff; border-inline-start: 3px solid var(--error); padding-inline-start: .75rem; overflow-wrap: anywhere; }
	.progress-panel { background: #212121; border-radius: .3rem; padding: 1rem; margin-block: 1rem; }
	.progress { display: flex; align-items: center; gap: .75rem; }
	progress { width: 100%; height: .5rem; accent-color: var(--neutral); margin-block: .75rem .25rem; }
	.details-status { color: #aaa; }
	.pagination { display: flex; align-items: center; justify-content: space-between; gap: .5rem; margin-top: 1rem; }
	.progress :global(.loading) { position: static; margin: 0; }
	.confirmation { width: min(28rem, 70vw); }
	.actions { justify-content: flex-end; margin-top: 1.5rem; }
</style>
