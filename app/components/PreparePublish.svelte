<script context="module">
	export const isPublishing = writable(false);
	export const remountAddonScroller = writable(false);
</script>

<script>
	import { _ } from 'svelte-i18n';
	import Modal from '../components/Modal.svelte';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import CloudUpload from '@lucide/svelte/icons/cloud-upload';
	import Cross from '@lucide/svelte/icons/x';
	import Folder from '@lucide/svelte/icons/folder';
	import LinkOut from '@lucide/svelte/icons/external-link';
	import { tippyFollow, tippy } from '../tippy';
	import * as dialog from '@tauri-apps/plugin-dialog';
	import { invoke } from '@tauri-apps/api/core';
	import { open } from '@tauri-apps/plugin-shell';
	import { playSound } from '../sounds';
	import FileBrowser from './FileBrowser.svelte';
	import BBCodeEditor from './BBCodeEditor.svelte';
	import { writable } from 'svelte/store';
	import { Transaction } from '../transactions';
	import { formatSize } from '../format.js';
	import Loading from './Loading.svelte';
	import { translateError } from '../i18n';
	import { Steam } from '../steam';
	import { onMount } from 'svelte';

	export let updatingAddon = null;
	export let preparePublish;

	function togglePreparePublish() {
		$preparePublish = !$preparePublish;
	}

	let gmaIcon;
	let gmaIconPath = null;
	let gmaIconBase64;
	let gmaEntries = writable([]);
	let gmaSize;
	let readyForPublish = false;
	let ignoreGlobs = AppSettings.ignore_globs;

	let titleInput;
	let addonTypeInput;
	let changes = '';
	let gmaNameInput;
	let gmaNameTouched = false;
	let description = '';
	let descriptionTouched = false;
	let descriptionError = null;
	let publishMode = AppSettings.workshop_update_mode;
	let savingPublishMode = false;
	let publishModeOpen = false;
	let publishActions;
	let publishModeButton;
	const descriptionMaxBytes = 7999;
	const descriptionEncoder = new TextEncoder();
	let activeTab = 'files';
	let descriptionFormattingOpen = false;
	let changesFormattingOpen = false;
	let editorHistoryKey = 0;
	let ignoreOpen = false;
	const tabs = ['files', 'description', 'changelog'];
	$: descriptionBytes = descriptionEncoder.encode(description).length;
	$: canUpdateDescription = !!$updatingAddon && descriptionTouched && descriptionError === null
		&& description !== ($updatingAddon.description ?? '');
	$: descriptionOnly = !!$updatingAddon && publishMode === 'description';
	$: canSubmit = !savingPublishMode && (descriptionOnly ? canUpdateDescription : readyForPublish);
	$: publishLabel = descriptionOnly ? $_('update_description') : $_($updatingAddon ? 'package_update' : 'package_publish');
	$: publishHelp = descriptionOnly ? $_('update_description_help') : $_($updatingAddon ? 'package_update_help' : 'package_publish_help');
	$: if (!$preparePublish || $isPublishing) publishModeOpen = false;
	$: if ($preparePublish) publishMode = AppSettings.workshop_update_mode;

	async function selectPublishMode(mode) {
		if ($isPublishing || savingPublishMode) return;
		publishMode = mode;
		publishModeOpen = false;
		publishModeButton.focus();
		if (mode === AppSettings.workshop_update_mode) return;
		savingPublishMode = true;
		try {
			await invoke('update_settings', { settings: { ...AppSettings, workshop_update_mode: mode } });
			AppSettings.workshop_update_mode = mode;
		} catch (error) {
			await dialog.message($_('remember_update_mode_error', { values: { error: String(error) } }), { kind: 'error' });
		} finally {
			savingPublishMode = false;
		}
	}

	function dismissPublishMode(event) {
		if (publishModeOpen && !publishActions.contains(event.target)) publishModeOpen = false;
	}

	function onPublishModeKeydown(event) {
		if (publishModeOpen && event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			publishModeOpen = false;
			publishModeButton.focus();
		}
	}

	function onDescriptionInput(event) {
		description = event.detail;
		descriptionTouched = true;
		descriptionError = descriptionEncoder.encode(description).length > descriptionMaxBytes
			? 'ERR_DESCRIPTION_TOO_LONG'
			: description.includes('\0') ? 'ERR_DESCRIPTION_CONTAINS_NUL' : null;
		checkForm();
	}

	function onTabKeydown(event, index) {
		let next;
		if (event.key === 'ArrowRight') next = (index + 1) % tabs.length;
		else if (event.key === 'ArrowLeft') next = (index + tabs.length - 1) % tabs.length;
		else if (event.key === 'Home') next = 0;
		else if (event.key === 'End') next = tabs.length - 1;
		else return;
		event.preventDefault();
		activeTab = tabs[next];
		event.currentTarget.parentElement.children[next].focus();
	}

	let upscale;
	let canUpscale = false;

	let pathInput;
	let pathInputContainer;
	let pathValue = '';
	let pathFailMessage = null;

	// The preview image the backend uploads when no icon is chosen: the user's Steam avatar
	let defaultIconUrl = '/img/steam_anonymous.jpg';
	function refreshDefaultIcon() {
		invoke('default_workshop_icon').then(icon => {
			if (icon) defaultIconUrl = 'data:image/png;base64,' + icon;
		}).catch(error => {
			defaultIconUrl = null;
			dialog.message(translateError(String(error)), { kind: 'error' });
		});
	}
	refreshDefaultIcon();
	$: if ($preparePublish) refreshDefaultIcon();
	function browseAddon() {
		dialog.open({ directory: true }).then(path => {
			if (path && path.length > 0) {
				onPathChanged(path);
			}
		});
	}

	function browseIcon() {
		dialog.open({

			filters: [{
				extensions: ['jpg', 'jpeg', 'png', 'gif'],
				name: $_('icon_file_pick')
			}]

		}).then(path => {
			if (path) {
				invoke('verify_icon', { path }).then(([base64, can_upscale]) => {
					canUpscale = can_upscale;
					gmaIconPath = path;
					gmaIconBase64 = base64;
				}, transactionId => new Transaction(transactionId, () => ''));
			}
		});
	}
	function removeIcon() {
		gmaIconPath = null;
		gmaIconBase64 = null;
		canUpscale = false;
	}

	function checkPath(path, successSound) {
		return invoke('verify_whitelist', { path }).then(([entries, size]) => {

			$gmaEntries = entries;
			gmaSize = size;

			pathFailMessage = null;
			tippyFollow(pathInputContainer, pathFailMessage);

			pathValue = path;

			if (successSound) playSound('success');

			checkForm();

		}, (err) => {

			$gmaEntries = [];
			pathValue = pathInput.value;
			pathFailMessage = translateError(err);

			tippyFollow(pathInputContainer, pathFailMessage);
			playSound('error');

			pathValue = path;

			checkForm();

		});
	}

	async function onPathChanged(path, playSound) {
		if (path.length > 0) {
			await checkPath(path, playSound);
		} else {
			pathFailMessage = null;
			tippyFollow(pathInputContainer, pathFailMessage);
			pathValue = '';
			checkForm(false);
		}
	}

	// Keeps the suggested .gma name in the same shape the backend accepts
	function gmaNameFromTitle(title) {
		return title
			.replace(/[\/\\:*?"<>|]/g, '')
			.replace(/\s+/g, ' ')
			.trim()
			.replace(/\.+$/, '')
			.trim();
	}

	function onTitleChanged() {
		if (!gmaNameTouched && gmaNameInput) {
			gmaNameInput.value = gmaNameFromTitle(titleInput.value);
		}
		checkForm();
	}

	function onGmaNameInput() {
		// Stop following the title once the user has typed their own name.
		// Clearing the field falls back to "publishedaddon.gma".
		gmaNameTouched = true;
	}

	function trackAddonTitle(title) {
		gmaNameTouched = false;
		if (gmaNameInput) gmaNameInput.value = gmaNameFromTitle(title ?? '');
	}

	let tagChoiceContainer;
	let chosenAddonTags = [null, null, null];
	const addonTags = ['fun', 'roleplay', 'scenic', 'movie', 'realism', 'cartoon', 'water', 'comic', 'build'];
	const addonTypes = ['ServerContent', 'gamemode', 'map', 'weapon', 'vehicle', 'npc', 'tool', 'effects', 'model', 'entity'];
	function tagChosen() {
		const chosen = [];
		tagChoiceContainer.querySelectorAll(':scope > .tag-choice').forEach((choice, i) => {
			if (choice.value !== 'default') {
				if (chosen.findIndex(choice => choice === choice.value) !== -1) {
					choice.value = 'default';
					chosen[i] = null;
				} else {
					chosen[i] = choice.value;
				}
			}
		});
		chosenAddonTags = chosen;
	}

	function openEntry(path) {
		invoke('open', { path: pathValue + PATH_SEPARATOR + path });
	}

	function openAddon() {
		invoke('open', { path: pathValue });
	}

	async function ignoreKeyPress(e) {
		if (e.which === 13 || e.keyCode === 13 || e.key === 'Enter') {
			e.preventDefault();
			const ignore = this.value.trim();
			if (ignore.length > 0 && AppSettings.ignore_globs.findIndex(s => s === ignore) === -1) {
				AppSettings.ignore_globs.push(ignore);
				ignoreGlobs = AppSettings.ignore_globs;
				await invoke('update_settings', { settings: AppSettings });
				if (pathValue.length > 0) checkPath(pathValue);
			}
			this.value = '';
		}
	}
	async function removeIgnore() {
		const ignore = this.innerText;
		const index = AppSettings.ignore_globs.findIndex(s => s === ignore);
		if (index !== -1) {
			AppSettings.ignore_globs.splice(index, 1);
			ignoreGlobs = AppSettings.ignore_globs;
			await invoke('update_settings', { settings: AppSettings });
			if (pathValue.length > 0) checkPath(pathValue);
		}
	}

	async function publish() {
		if (savingPublishMode) return;
		if (descriptionOnly) return publishDescription();
		if (!readyForPublish || $isPublishing) return;
		const publishingAddon = $updatingAddon;
		const descriptionUpdate = descriptionTouched ? description : null;
		$isPublishing = true;
		playSound('success');

		invoke('publish', {
			request: {
				contentPathSrc: pathValue,
				title: titleInput.value.trim(),
				description: descriptionUpdate,
				tags: chosenAddonTags.filter(tag => !!tag),
				addonType: addonTypeInput.value,
				gmaName: gmaNameInput.value.trim(),
				iconPath: gmaIconPath,
				upscale: canUpscale && upscale.checked,
				updateId: publishingAddon?.id,
				changes: changes || null,
			},
		}).then(transactionId => {
			const transaction = new Transaction(transactionId, transaction => {
				return $_(transaction.status ?? 'PUBLISH_PACKING', { values: {
					pct: transaction.progress,
					data: formatSize((transaction.progress / 100) * gmaSize),
					dataTotal: formatSize(gmaSize)
				}});
			});

			transaction.listen(event => {
				if (event.finished) {
					if (descriptionUpdate !== null) {
						if (publishingAddon) publishingAddon.description = descriptionUpdate;
						if (publishingAddon && $updatingAddon === publishingAddon && description === descriptionUpdate) descriptionTouched = false;
					}
					$remountAddonScroller = true;
					Steam.MyWorkshop = [];
				}

				if (event.finished || event.error || event.cancelled) {
					$isPublishing = false;
				}
			});
		}).catch(async error => {
			$isPublishing = false;
			await dialog.message(translateError(String(error)), { kind: 'error' });
		});
	}

	async function publishDescription() {
		if (!canUpdateDescription || $isPublishing) return;
		const publishingAddon = $updatingAddon;
		const descriptionUpdate = description;
		const historyKey = editorHistoryKey;
		$isPublishing = true;

		try {
			const transactionId = await invoke('publish_description', {
				addonId: publishingAddon.id,
				description: descriptionUpdate,
			});
			const transaction = new Transaction(transactionId, () => $_('PUBLISH_UPDATING_DESCRIPTION'));
			transaction.listen(event => {
				if (event.finished) {
					publishingAddon.description = descriptionUpdate;
					if ($updatingAddon === publishingAddon && editorHistoryKey === historyKey && description === descriptionUpdate) descriptionTouched = false;
					$remountAddonScroller = true;
					Steam.MyWorkshop = [];
					playSound('success');
				}
				if (event.finished || event.error || event.cancelled) $isPublishing = false;
			});
		} catch (error) {
			$isPublishing = false;
			await dialog.message(translateError(String(error)), { kind: 'error' });
		}
	}

	async function openChangeNotes(event) {
		try {
			await open(event.currentTarget.href);
		} catch (error) {
			await dialog.message(String(error), { kind: 'error' });
		}
	}

	function isFormValid() {
		if (descriptionError) return false;
		if (pathValue.length === 0 || pathFailMessage !== null) return false;

		let chosenAddonTag = false;
		for (let i = 0; i < chosenAddonTags.length; i++) {
			if (chosenAddonTags[i] !== null) {
				chosenAddonTag = true;
				break;
			}
		}
		if (!chosenAddonTag) return false;

		if (titleInput.value.trim().length === 0) return false;

		if (addonTypeInput.value === 'default') return false;

		return true;
	}

	function checkForm(sound) {
		const isValid = isFormValid();
		if (isValid !== readyForPublish) {
			readyForPublish = isValid;
			if (sound !== false) {
				if (readyForPublish) {
					playSound('btn-on');
				} else {
					playSound('btn-off');
				}
			}
		}
	}

	const tagSearchMax = Math.max(addonTypes.length, addonTags.length);
	onMount(() => updatingAddon.subscribe(async updatingAddon => {
		editorHistoryKey += 1;
		changes = '';
		activeTab = 'files';
		descriptionFormattingOpen = false;
		changesFormattingOpen = false;
		ignoreOpen = false;
		description = updatingAddon?.description ?? '';
		descriptionTouched = false;
		descriptionError = null;

		if (!updatingAddon) {
			gmaIconPath = null;
			gmaIconBase64 = null;
			$gmaEntries = [];
			gmaSize = null;
			readyForPublish = false;
			titleInput.value = '';
			addonTypeInput.value = 'default';
			upscale.checked = AppSettings.upscale_addon_icon;
			upscale = upscale;
			canUpscale = false;
			pathInput.value = '';
			pathValue = '';
			pathFailMessage = null;
			chosenAddonTags = [null, null, null];
			trackAddonTitle('');
			return;
		}

		canUpscale = false;

		gmaIconPath = null;
		gmaIconBase64 = updatingAddon.previewUrl;

		if (updatingAddon.id in AppSettings.my_workshop_local_paths) {
			await onPathChanged(AppSettings.my_workshop_local_paths[updatingAddon.id]);
		} else {
			pathValue = '';
			pathFailMessage = null;
		}

		addonTypeInput.value = 'default';
		addonTypeInput = addonTypeInput;

		chosenAddonTags = [null, null, null];
		let chosen = 0;
		for (let i = 0; i < updatingAddon.tags.length; i++) {
			if (updatingAddon.tags[i] === 'ServerContent') {
				addonTypeInput.value = 'ServerContent';
				addonTypeInput = addonTypeInput;
				continue;
			}
			if (updatingAddon.tags[i] === 'Addon') continue;

			const tag = updatingAddon.tags[i].toLowerCase();
			let search = -1;
			while (++search < tagSearchMax) {
				if (addonTags[search] === tag) {
					chosenAddonTags[chosen++] = tag;
				} else if (addonTypes[search] === tag) {
					addonTypeInput.value = tag;
					addonTypeInput = addonTypeInput;
				}
			}
		}
		chosenAddonTags = chosenAddonTags;

		titleInput.value = updatingAddon.title;
		titleInput = titleInput;

		trackAddonTitle(updatingAddon.title);

		checkForm(false);
	}));

	async function publishIcon() {
		if ($isPublishing || !gmaIconPath || !$updatingAddon) return;
		$isPublishing = true;
		playSound('success');

		invoke('publish_icon', {

			iconPath: gmaIconPath,
			upscale: canUpscale && upscale.checked,
			addonId: (await $updatingAddon).id,

		}).then(transactionId => {
			const transaction = new Transaction(transactionId, transaction => {
				return $_(transaction.status ?? 'PUBLISH_PROCESSING_ICON', { values: {
					pct: transaction.progress,
					data: formatSize((transaction.progress / 100) * gmaSize),
					dataTotal: formatSize(gmaSize)
				}});
			});

			transaction.listen(event => {
				if (event.finished) {
					$remountAddonScroller = true;
					Steam.MyWorkshop = [];
				}

				if (event.finished || event.error || event.cancelled) {
					$isPublishing = false;
				}
			});
		}).catch(async error => {
			$isPublishing = false;
			await dialog.message(translateError(String(error)), { kind: 'error' });
		});
	}
</script>

<svelte:window on:click={dismissPublishMode} on:focusin={dismissPublishMode} on:keydown|capture={onPublishModeKeydown}/>

<Modal id="prepare-publish" active={$preparePublish} cancel={togglePreparePublish}>
	<div id="details-container" class="hide-scroll">
		{#if $updatingAddon}
			<div id="ws-link"><a class="color" href="https://steamcommunity.com/sharedfiles/filedetails/?id={$updatingAddon.id}" target="_blank">{$_('workshop_page')}<LinkOut class="icon" size=".8rem"/></a></div>
		{/if}

		{#if gmaIconBase64}
			<div id="icon-container" on:click={browseIcon}>
				<div id="addon-icon-background" style="background-image: url('{gmaIconBase64}')"></div>
				{#if canUpscale && upscale.checked}
					<div id="addon-icon" class="upscale">
						<img src={gmaIconBase64} bind:this={gmaIcon}/>
						<img src={defaultIconUrl}/>
					</div>
				{:else}
					<div id="addon-icon">
						<img src={gmaIconBase64} bind:this={gmaIcon}/>
					</div>
				{/if}
			</div>
		{:else}
			<div id="icon-container" on:click={browseIcon} use:tippy={$_('default_icon_tip')}>
				<div id="addon-icon-background" style="background-image: url('{defaultIconUrl}')"></div>
				<div id="addon-icon"><img src={defaultIconUrl} bind:this={gmaIcon}/></div>
			</div>
		{/if}
		<div id="icon-browse-container">
			{#if gmaIconBase64 && !$updatingAddon}
				<div id="icon-browse" on:click={removeIcon}><Cross class="icon" size="1rem"/>{$_('remove_icon')}</div>
			{:else}
				<div id="icon-browse" on:click={browseIcon}><Folder class="icon" size="1rem"/>{$_('browse')}</div>
			{/if}
			{#if $updatingAddon && !$isPublishing}
				<div id="icon-publish" on:click={publishIcon} use:tippy={$_('publish_icon')} class:disabled={gmaIconPath == null}><CloudUpload class="icon" size="1rem"/></div>
			{/if}
		</div>
		<p>{$_('icon_instructions')}</p>
		<div id="upscale-container">
			<label class:disabled={!canUpscale}>
				<input type="checkbox" id="upscale" bind:this={upscale} checked={AppSettings.upscale_addon_icon} disabled={!canUpscale} on:change={() => upscale = upscale}/>
				{$_('upscale_addon_icon')}
			</label>
		</div>

		<div class="path-container" bind:this={pathInputContainer}>
			<input type="text" class:error={pathFailMessage?.length > 0} bind:this={pathInput} id="path" placeholder={$_('addon_path')} required on:change={() => onPathChanged(pathInput.value, true)} value={pathValue}/>
			<div class="browse icon-button" on:click={browseAddon}><Folder class="icon" size="1rem"/></div>
		</div>

		<span use:tippy={$updatingAddon ? $_('update_addon_title_via_steam') : null}>
			<input type="text" id="title" placeholder={$_('addon_title')} disabled={$updatingAddon} bind:this={titleInput} on:input={onTitleChanged} on:change={onTitleChanged}/>
		</span>

		<div class="path-container" id="gma-name-container" use:tippy={$_('gma_file_name_tip')}>
			<input type="text" id="gma-name" placeholder={$_('gma_file_name_placeholder')} bind:this={gmaNameInput} on:input={onGmaNameInput}/>
			<div class="extension">.gma</div>
		</div>

		<select id="addon-type" bind:this={addonTypeInput} on:blur={checkForm} on:change={checkForm}>
			<option value="default" selected hidden disabled>{$_('addon_type')}</option>
			{#each addonTypes as addonType}
				<option value={addonType}>{$_('addon_types.' + addonType)}</option>
			{/each}
		</select>

		<div id="addon-tags" on:blur={checkForm} on:change={checkForm} bind:this={tagChoiceContainer}>
			<select on:change={tagChosen} class="tag-choice" value={chosenAddonTags[0] ?? 'default'}>
				<option value="default">{$_('tag_1')}</option>
				{#each addonTags as tag}
					{#if chosenAddonTags.findIndex(choice => choice === tag) === -1}
						<option value={tag}>{$_('addon_tags.' + tag)}</option>
					{:else}
						<option value={tag} disabled>{$_('addon_tags.' + tag)}</option>
					{/if}
				{/each}
			</select>
			<select on:change={tagChosen} class="tag-choice" value={chosenAddonTags[1] ?? 'default'}>
				<option value="default">{$_('tag_2')}</option>
				{#each addonTags as tag}
					{#if chosenAddonTags.findIndex(choice => choice === tag) === -1}
						<option value={tag}>{$_('addon_tags.' + tag)}</option>
					{:else}
						<option value={tag} disabled>{$_('addon_tags.' + tag)}</option>
					{/if}
				{/each}
			</select>
			<select on:change={tagChosen} class="tag-choice" value={chosenAddonTags[2] ?? 'default'}>
				<option value="default">{$_('tag_3')}</option>
				{#each addonTags as tag}
					{#if chosenAddonTags.findIndex(choice => choice === tag) === -1}
						<option value={tag}>{$_('addon_tags.' + tag)}</option>
					{:else}
						<option value={tag} disabled>{$_('addon_tags.' + tag)}</option>
					{/if}
				{/each}
			</select>
		</div>

		<div id="publish-actions" bind:this={publishActions}>
			<button type="button" id="publish-btn" on:click={publish} disabled={!canSubmit || $isPublishing} aria-describedby="publish-help">
				{#if $isPublishing}
					<Loading size="1.1rem"/>
				{:else}
					<CloudUpload class="icon" size="1.1rem"/>
				{/if}
				<span>{publishLabel}</span>
			</button>
			{#if $updatingAddon}
				<button type="button" id="publish-mode-button" bind:this={publishModeButton} on:click={() => publishModeOpen = !publishModeOpen} disabled={$isPublishing || savingPublishMode} aria-label={$_('choose_update_mode')} aria-expanded={publishModeOpen} aria-controls="publish-modes">
					<ChevronDown class="icon" size="1rem"/>
				</button>
				<div id="publish-modes" role="group" aria-label={$_('choose_update_mode')} hidden={!publishModeOpen}>
					<button type="button" aria-pressed={publishMode === 'description'} on:click={() => selectPublishMode('description')} disabled={$isPublishing || savingPublishMode}>{$_('description_only')}</button>
					<button type="button" aria-pressed={publishMode === 'package'} on:click={() => selectPublishMode('package')} disabled={$isPublishing || savingPublishMode}>{$_('package_update')}</button>
				</div>
			{/if}
		</div>
		<p id="publish-help" aria-live="polite">{publishHelp}</p>
	</div>

	<div id="publish-workspace">
		<div class="workspace-tabs" role="tablist" aria-label={$_('publish_tabs.label')}>
			{#each tabs as tab, index}
				<button type="button" role="tab" id={`publish-tab-${tab}`} aria-controls={`publish-panel-${tab}`} aria-selected={activeTab === tab} tabindex={activeTab === tab ? 0 : -1} class:invalid={tab === 'description' && descriptionError !== null} on:click={() => activeTab = tab} on:keydown={event => onTabKeydown(event, index)}>{$_('publish_tabs.' + tab)}</button>
			{/each}
		</div>
		<div id="publish-panel-files" class="workspace-panel files-panel" role="tabpanel" aria-labelledby="publish-tab-files" tabindex="0" hidden={activeTab !== 'files'}>
			<FileBrowser fileSelect={path => onPathChanged(path)} dropActive={$preparePublish && activeTab === 'files' && !$isPublishing} background={true} browsePath={pathValue.length > 0 ? pathValue : null} entriesList={gmaEntries} {openEntry} open={openAddon}/>
			<details id="ignore" bind:open={ignoreOpen}>
				<summary><span class="ignore-chevron"><ChevronRight class="icon" size=".85rem"/></span>{$_('ignored_file_patterns')}</summary>
				<div class="ignore-content">
					<input type="text" aria-label={$_('ignored_file_patterns')} placeholder={$_('add_ellipsis')} on:keypress={ignoreKeyPress}/>
					<div class="ignore-patterns">
						{#each ignoreGlobs as ignore}
							<button type="button" on:click={removeIgnore}>{ignore}</button>
						{/each}
						{#each window.DEFAULT_IGNORE_GLOBS as ignore}
							<div class="default" use:tippyFollow={$_('ignored_for_convenience')}>{ignore}</div>
						{/each}
					</div>
				</div>
			</details>
		</div>
		<div id="publish-panel-description" class="workspace-panel" role="tabpanel" aria-labelledby="publish-tab-description" tabindex="0" hidden={activeTab !== 'description'}>
			<BBCodeEditor id="description" label={$_('workshop_description')} value={description} on:input={onDescriptionInput} disabled={$isPublishing} error={descriptionError} help={$_('workshop_description_help')} size={$_('workshop_description_size', { values: { bytes: descriptionBytes, max: descriptionMaxBytes } })} bind:formattingOpen={descriptionFormattingOpen} active={$preparePublish && activeTab === 'description'} historyKey={editorHistoryKey}/>
		</div>
		<div id="publish-panel-changelog" class="workspace-panel" role="tabpanel" aria-labelledby="publish-tab-changelog" tabindex="0" hidden={activeTab !== 'changelog'}>
			<BBCodeEditor id="changes" label={$_('changelog_optional')} bind:value={changes} disabled={$isPublishing} bind:formattingOpen={changesFormattingOpen} active={$preparePublish && activeTab === 'changelog'} historyKey={editorHistoryKey}/>
			{#if $updatingAddon}
				<div class="editor-footer">
					<a href={`https://steamcommunity.com/sharedfiles/filedetails/changelog/${$updatingAddon.id}`} on:click|preventDefault={openChangeNotes}>{$_('view_edit_change_notes')}<LinkOut class="icon" size=".85rem"/></a>
				</div>
			{/if}
		</div>
	</div>
</Modal>

<style>
	:global(#prepare-publish > div) {
		display: flex;
		width: 70rem;
		min-height: 0;
		height: 50rem;
		padding: 1.5rem;
	}
	@media (max-width: 70rem), (max-height: 44rem) {
		:global(#prepare-publish > .hide-scroll) {
			width: 100%;
			height: 100%;
			margin: 0;
			max-width: 100%;
			max-height: 100%;
		}
	}
	#details-container {
		width: 18rem;
		flex-shrink: 0;
		min-height: 0;
		display: flex;
		flex-direction: column;
	}
	#details-container > * {
		flex-shrink: 0;
	}
	#details-container > #icon-container {
		flex: 0 1 15rem;
		min-height: 4rem;
	}
	#publish-workspace {
		flex: 1;
		margin-left: 1.5rem;
		display: flex;
		flex-direction: column;
		min-width: 0;
		min-height: 0;
		gap: 1rem;
	}
	.workspace-tabs {
		display: flex;
		border-bottom: 1px solid #414141;
	}
	.workspace-tabs button {
		flex: 1;
		padding: .7rem;
		font: inherit;
		color: #aaa;
		border: 0;
		border-bottom: 2px solid transparent;
		background: transparent;
		cursor: pointer;
	}
	.workspace-tabs button[aria-selected='true'] {
		color: #fff;
		background: #252525;
		border-bottom-color: #fff;
	}
	.workspace-tabs button:hover {
		background: #313131;
	}
	.workspace-tabs button.invalid {
		color: #ff7777;
	}
	.workspace-tabs button:focus-visible, summary:focus-visible, .workspace-panel:focus-visible {
		outline: 2px solid #127cff;
		outline-offset: -2px;
	}
	.workspace-panel {
		flex: 1;
		min-height: 0;
		min-width: 0;
		display: flex;
		flex-direction: column;
		overflow: auto;
	}
	.workspace-panel[hidden] {
		display: none;
	}
	.editor-footer {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		flex-wrap: wrap;
		gap: .5rem;
		padding-top: .75rem;
		flex-shrink: 0;
		font-size: .8em;
	}
	.editor-footer a {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: .5rem;
		padding: .6rem .75rem;
		border: 1px solid #414141;
		border-radius: 4px;
		background: #313131;
		color: #fff;
		font: inherit;
		text-decoration: none;
		cursor: pointer;
	}
	.editor-footer a:hover {
		background: #414141;
	}
	.editor-footer a:focus-visible {
		outline: 2px solid #127cff;
		outline-offset: 1px;
	}
	.files-panel {
		gap: .75rem;
	}
	.files-panel > :global(#file-browser) {
		border-radius: .4rem;
		overflow: hidden;
		flex: 1 0 12rem;
	}

	input[type='text'] {
		appearance: none;
		font: inherit;
		border-radius: 4px;
		border: none;
		background: rgba(255,255,255,.1);
		box-shadow: 0px 0px 2px 0px rgba(0, 0, 0, .4);
		padding: .7rem;
		color: #fff;
		font-size: .85em;
		width: 100%;
	}
	input[type='text'][disabled] {
		opacity: 0.5;
	}
	input[type='text']:focus {
		box-shadow: inset 0 0 0px 1.5px #127cff;
		outline: none;
	}
	input[type='text'].error {
		outline: none !important;
		box-shadow: inset 0 0 0px 1.5px var(--error) !important;
	}
	.path-container {
		display: flex;
	}
	.path-container > input {
		flex: 1;
		min-width: 0;
	}
	#gma-name-container > .extension {
		display: flex;
		align-items: center;
		margin-left: .75rem;
		padding: .7rem;
		font-size: .85em;
		opacity: .6;
	}
	.browse {
		margin-left: .75rem;
		padding: .7rem;
		width: 2.4rem;
		height: 2.4rem;
		display: flex;
	}
	#icon-container {
		position: relative;
		overflow: hidden;
		cursor: pointer;
		background-color: #101010;
		box-shadow: inset 0 0 6px 2px rgb(0 0 0 / 20%);
		border: 1px solid #101010;
		border-radius: .4rem;
	}
	#icon-browse-container {
		display: flex;
	}
	#icon-browse {
		flex: 1;
	}
	#icon-browse, #icon-publish {
		cursor: pointer;
		background: #313131;
		box-shadow: 0px 0px 2px 0px rgb(0 0 0 / 40%);
		border-radius: 4px;
		padding: .7rem;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	#icon-publish {
		margin-left: .5rem;
		background-color: var(--neutral);
		transition: background-color .5s;
	}
	#icon-publish.disabled {
		cursor: default;
		background-color: #313131;
	}
	#icon-browse:active, #icon-publish:active {
		background: #252525;
	}
	#icon-browse > :global(.icon) {
		margin-right: .5rem;
	}
	#addon-icon {
		width: 100%;
		height: 100%;
		position: relative;
		max-height: 100%;
	}
	#addon-icon img {
		background-image: url('/img/transparency.svg');
		margin: auto;
		top: 0;
		left: 0;
		right: 0;
		bottom: 0;
		max-width: 100%;
		max-height: 100%;
		display: block;
	}
	#addon-icon:not(.upscale) img {
		position: absolute;
	}
	#addon-icon.upscale {
		width: max-content;
		position: relative;
		display: block;
		margin: auto;
	}
	#addon-icon.upscale img {
		height: 100%;
	}
	#addon-icon.upscale > img:first-child {
		position: absolute;
		width: 100%;
	}
	#addon-icon.upscale > img:last-child {
		opacity: 0;
	}
	#addon-icon-background {
		position: absolute;
		width: calc(100% + 10px);
		height: calc(100% + 10px);
		background-size: cover;
		background-position: 50% 50%;
		filter: blur(5px);
		left: -5px;
		top: -5px;
		z-index: 0;
	}

	p {
		white-space: pre-line;
		line-height: 1.4;
		margin-top: 0;
		margin-bottom: 0;
		text-align: center;
	}

	#addon-type {
		display: block;
	}

	#addon-tags {
		display: flex;
	}
	#addon-tags select {
		flex: 1;
		flex-basis: 0;
		min-width: 0;
	}

	#details-container > *:not(:last-child) {
		margin-bottom: .6rem;
	}

	select {
		-webkit-appearance: none;
		-moz-appearance: none;
		appearance: none;
		font: inherit;
		border-radius: 4px;
		border: none;
		background: rgba(255,255,255,.1);
		box-shadow: 0px 0px 2px 0px rgb(0 0 0 / 40%);
		padding: .7rem;
		color: #fff;
		font-size: .85em;
		width: 100%;
		cursor: pointer;
		text-align: center;
		text-align-last: center;
	}
	select:focus {
		box-shadow: inset 0 0 0px 1.5px #127cff;
		outline: none;
	}
	option {
		background: #313131;
		color: #fff;
	}
	option:hover {
		background: #CECECE;
		color: #313131;
	}
	select:nth-child(2) {
		margin-left: 1rem;
		margin-right: 1rem;
	}

	#ignore {
		flex-shrink: 0;
	}
	#ignore summary {
		display: flex;
		align-items: center;
		gap: .5rem;
		list-style: none;
		padding: .7rem;
		border-radius: 4px;
		background: #313131;
		cursor: pointer;
		font-size: .85em;
	}
	#ignore summary::-webkit-details-marker {
		display: none;
	}
	.ignore-chevron {
		display: inline-flex;
		transition: transform .15s;
	}
	#ignore[open] .ignore-chevron {
		transform: rotate(90deg);
	}
	.ignore-content {
		padding-top: .75rem;
	}
	.ignore-patterns {
		overflow: auto;
		max-height: 10rem;
		margin-top: .75rem;
		background-color: #292929;
		box-shadow: inset 0 0 6px 2px rgb(0 0 0 / 20%);
		border: 1px solid #101010;
		border-radius: .4rem;
	}
	.ignore-patterns > * {
		display: block;
		width: 100%;
		border: 0;
		border-radius: 0;
		background: transparent;
		color: #fff;
		font: inherit;
		padding: .6rem;
		font-size: .9em;
		text-align: left;
		transition: background-color .1s;
		word-break: break-all;
	}
	.ignore-patterns > button {
		cursor: pointer;
	}
	.ignore-patterns > .default {
		color: rgba(255,255,255,.5);
	}
	.ignore-patterns > :nth-child(2n-1) {
		background-color: rgb(0, 0, 0, .12);
	}

	#publish-actions {
		display: flex;
		position: relative;
	}
	#publish-actions button {
		font: inherit;
		color: #fff;
		border: 0;
		cursor: pointer;
	}
	#publish-btn, #publish-mode-button {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: .4rem;
		padding: .7rem;
		text-align: center;
		background-color: var(--neutral);
		box-shadow: 0 0 5px rgba(0, 0, 0, .1);
		text-shadow: 0px 1px 0px rgba(0, 0, 0, .6);
		line-height: 1.2;
		border-radius: 4px;
		transition: background-color .5s;
	}
	#publish-btn {
		flex: 1;
		min-width: 0;
	}
	#publish-btn:not(:last-child) {
		border-radius: 4px 0 0 4px;
	}
	#publish-mode-button {
		flex-shrink: 0;
		border-radius: 0 4px 4px 0;
	}
	#publish-actions #publish-mode-button {
		border-left: 1px solid rgba(0, 0, 0, .3);
	}
	#publish-actions button:disabled {
		background-color: #313131;
		color: #aaa;
		cursor: default;
	}
	#publish-actions button:focus-visible {
		outline: 2px solid #127cff;
		outline-offset: -2px;
	}
	#publish-modes {
		position: absolute;
		bottom: calc(100% + .4rem);
		left: 0;
		right: 0;
		z-index: 4;
		padding: .25rem;
		border: 1px solid #555;
		border-radius: 4px;
		background: #252525;
		box-shadow: 0 2px 10px rgba(0, 0, 0, .4);
	}
	#publish-modes button {
		display: block;
		width: 100%;
		padding: .7rem;
		text-align: left;
		border-radius: 2px;
		background: transparent;
	}
	#publish-modes button[aria-pressed='true'] {
		background: #414141;
		box-shadow: inset 3px 0 var(--neutral);
	}
	#publish-modes button:hover {
		background: #505050;
	}
	#publish-help {
		font-size: .8em;
		color: #aaa;
		text-align: left;
	}

	#upscale-container > label.disabled {
		opacity: .5;
	}
	#upscale-container > label:not(.disabled) {
		cursor: pointer;
	}
	#upscale-container > label {
		display: flex;
		justify-content: center;
	}
	#upscale-container input {
		margin-right: .5rem;
	}

	@media (prefers-reduced-motion: reduce) {
		.ignore-chevron {
			transition: none;
		}
	}

	#ws-link {
		text-align: center;
	}
	#ws-link :global(.icon) {
		margin-left: .2rem;
	}
</style>
