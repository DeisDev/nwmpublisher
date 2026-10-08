<script>
	import { tippy } from '../tippy';
	import { getLocaleFromNavigator, _ } from 'svelte-i18n';
	import Gear from '@lucide/svelte/icons/settings';
	import Modal from './Modal.svelte';
	import Sidebar from './Sidebar.svelte';
	import Setting from './Setting.svelte';
	import ChangelogDefaults from './ChangelogDefaults.svelte';
	import { playSound } from '../sounds';
	import { invoke } from '@tauri-apps/api/core';
	import { switchLanguage } from '../i18n';
	import { applyTheme } from '../theme';
	import { saveSettings, settingsSave, settings } from '../settings.js';

	let active = false;
	function toggle() {
		active = !active;
	}

	let activeItem = 'general';
	const sections = [
		['general', 'settings.general.general'],
		['editors', 'settings.editors.title'],
		['changelog', 'changelog_defaults.nav'],
		['paths', 'settings.paths.paths'],
		['appearance', 'settings.appearance'],
	];
	const layouts = [['vertical', ['bbcode.vertical']], ['horizontal', ['bbcode.horizontal']]];

	function preventSubmit(e) {
		e.preventDefault();
		// Field changes are saved individually by their controls.
	}

	async function validateGmod(before, after) {
		if (after.trim().length > 0) {
			if (!(await invoke('validate_gmod', { path: after }))) {
				return before.trim().length > 0 ? before : null;
			} else {
				playSound('success');
			}
		}
		return after;
	}

	let form;
	function afterChange() {
		if (this.type === 'checkbox') {
			AppSettings[this.id] = this.checked;
		} else if (typeof this.value === 'string') {
			const trimmed = this.value.trim();
			AppSettings[this.id] = trimmed.length > 0 ? trimmed : null;
		} else {
			AppSettings[this.id] = this.value;
		}
		saveSettings({ [this.id]: AppSettings[this.id] }).catch(() => {});
	}

	function afterChangeColor() {
		AppSettings[this.id] = parseInt(this.value.substr(1), 16);
		saveSettings({ [this.id]: AppSettings[this.id] }).catch(() => {});
	}

	function changeCustomColor() {
		afterChangeColor.call(this);
		updateCustomColor(this.id.substr('color_'.length), AppSettings[this.id]);
	}

	const languages = [
		['default', 'Automatic'],
		['en', 'English'],
	];
	for (let lang in window.APP_LANGUAGES) {
		if (lang === 'en') continue;
		languages.push([lang, window.APP_LANGUAGES[lang]?.LANGUAGE_NAME ?? lang]);
	}
	function chooseLanguage() {
		if (this.value === 'default') {
			AppSettings.language = null;
			switchLanguage(getLocaleFromNavigator());
		} else {
			AppSettings.language = this.value;
			switchLanguage(this.value);
		}
		saveSettings({ language: AppSettings.language }).catch(() => {});
	}

	const themes = [
		['system', ['settings.theme.system']],
		['dark', ['settings.theme.dark']],
		['light', ['settings.theme.light']],
	];
	function chooseTheme() {
		afterChange.call(this);
		applyTheme(AppSettings.theme);
	}

	const extractOverwriteModes = [
		['Overwrite', ['settings.extract_overwrite_mode.overwrite']],
		['Recycle', ['settings.extract_overwrite_mode.recycle']],
		['Delete', ['settings.extract_overwrite_mode.delete']]
	];
</script>

<Modal id="settings" active={active} cancel={toggle}>
	<Sidebar id="settings-sidebar">

		<h2>{$_('settings.settings')}</h2>
		<nav aria-label={$_('settings.settings')}>
			{#each sections as [section, label]}
				<button type="button" class:active={activeItem === section} aria-current={activeItem === section ? 'page' : undefined} on:click={() => activeItem = section}>{$_(label)}</button>
			{/each}
		</nav>

	</Sidebar>

	<form id="content" class="hide-scroll" on:submit={preventSubmit} bind:this={form}>
		<h2>{$_(sections.find(([section]) => section === activeItem)[1])}</h2>
		<p class="save-status" role="status">{$_('settings_save_' + $settingsSave.state)}</p>
		{#if $settingsSave.error}<p role="alert">{$settingsSave.error}</p>{/if}
		{#if activeItem === 'general'}
			<div id="open-count">
				<div>
					<Setting id="language" type="select" value={AppSettings.language ?? 'default'} choices={languages} afterChange={chooseLanguage}>{$_('settings.language')}</Setting>
					<Setting {afterChange} id="extract_overwrite_mode" type="select" value={AppSettings.extract_overwrite_mode} choices={extractOverwriteModes} tooltip={$_('settings.extract_overwrite_mode.tooltip')}>{$_('settings.extract_overwrite_mode.extract_overwrite_mode')}</Setting>
					<Setting {afterChange} id="sounds" type="bool" value={AppSettings.sounds}>{$_('settings.general.sounds')}</Setting>
					<Setting {afterChange} id="open_workshop_after_publish" type="bool" value={AppSettings.open_workshop_after_publish}>{$_('settings.general.open_workshop_after_publish')}</Setting>
					<Setting {afterChange} id="open_folder_after_extract" type="bool" value={AppSettings.open_folder_after_extract} tooltip={$_('settings.general.open_folder_after_extract_tooltip')}>{$_('settings.general.open_folder_after_extract')}</Setting>
				</div>
				<div>{$_('open_count', { values: { count: AppData.open_count } })}</div>
			</div>
		{:else if activeItem === 'editors'}
			<section>
				<h3>{$_('settings.editors.shared')}</h3>
				<Setting {afterChange} id="bbcode_syntax_highlighting" type="bool" value={$settings.bbcode_syntax_highlighting}>{$_('settings.editors.highlighting')}</Setting>
				<Setting {afterChange} id="bbcode_autocomplete" type="bool" value={$settings.bbcode_autocomplete} tooltip={$_('settings.editors.autocomplete_help')}>{$_('settings.editors.autocomplete')}</Setting>
				<Setting {afterChange} id="bbcode_indent" type="bool" value={$settings.bbcode_indent} tooltip={$_('settings.editors.indent_help')}>{$_('settings.editors.indent')}</Setting>
				<Setting {afterChange} id="bbcode_auto_close_tags" type="bool" value={$settings.bbcode_auto_close_tags} tooltip={$_('settings.general.bbcode_auto_close_tags_tooltip')}>{$_('settings.general.bbcode_auto_close_tags')}</Setting>
				<Setting {afterChange} id="bbcode_wrap_selection" type="bool" value={$settings.bbcode_wrap_selection} tooltip={$_('settings.general.bbcode_wrap_selection_tooltip')}>{$_('settings.general.bbcode_wrap_selection')}</Setting>
				<Setting {afterChange} id="bbcode_preview_layout" type="select" value={$settings.bbcode_preview_layout} choices={layouts}>{$_('bbcode.layout')}</Setting>
			</section>
			<section>
				<h3>{$_('settings.editors.description')}</h3>
				<Setting {afterChange} id="bbcode_convert_pasted_markdown" type="bool" value={AppSettings.bbcode_convert_pasted_markdown} tooltip={$_('settings.general.bbcode_convert_pasted_markdown_tooltip')}>{$_('settings.general.bbcode_convert_pasted_markdown')}</Setting>
			</section>
		{:else if activeItem === 'changelog'}
			<section>
				<Setting {afterChange} id="bbcode_convert_pasted_changelogs" type="bool" value={AppSettings.bbcode_convert_pasted_changelogs} tooltip={$_('settings.general.bbcode_convert_pasted_changelogs_tooltip')}>{$_('settings.general.bbcode_convert_pasted_changelogs')}</Setting>
			</section>
			<ChangelogDefaults id="global-changelog" global active={active}/>
		{:else if activeItem === 'paths'}
			<Setting {afterChange} id="gmod" type="directory" initial={AppData.gmod_dir ?? $_('ERR_UNKNOWN')} value={AppSettings.gmod} beforeChange={validateGmod}>{$_('settings.paths.gmod')}</Setting>
			<Setting {afterChange} id="downloads" type="directory" initial={AppData.downloads_dir} value={AppSettings.downloads}>{$_('settings.paths.downloads')}</Setting>
			<Setting {afterChange} id="user_data" type="directory" initial={AppData.user_data_dir} value={AppSettings.user_data}>{$_('settings.paths.user_data')}</Setting>
			<Setting {afterChange} id="temp" type="directory" initial={AppData.temp_dir} value={AppSettings.temp}>{$_('settings.paths.temp')}</Setting>
		{:else if activeItem === 'appearance'}
			<Setting id="theme" type="select" value={AppSettings.theme} choices={themes} afterChange={chooseTheme}>{$_('settings.theme.theme')}</Setting>
			<Setting afterChange={changeCustomColor} id="color_neutral" type="color" value={AppSettings.color_neutral}>{$_('settings.accessibility.color_neutral')}</Setting>
			<Setting afterChange={changeCustomColor} id="color_success" type="color" value={AppSettings.color_success}>{$_('settings.accessibility.color_success')}</Setting>
			<Setting afterChange={changeCustomColor} id="color_error" type="color" value={AppSettings.color_error}>{$_('settings.accessibility.color_error')}</Setting>

		{/if}
	</form>
</Modal>

<span class="nav-icon" use:tippy={$_('settings.settings')} on:click={toggle}><Gear class="icon" size="1.5rem" stroke-width="1.5" id="settings"/></span>

<style>
	h2 { margin: 0 0 1rem; font-size: 1.1rem; }
	h3 { margin: 0 0 1.25rem; font-size: .95rem; }
	.save-status { margin: 0 0 1.5rem; font-size: .8rem; color: var(--text-muted); }
	section { padding-bottom: 1.5rem; }
	section + section { padding-top: 1.5rem; border-top: 1px solid var(--border); }
	nav { display: flex; flex-direction: column; gap: .4rem; }
	nav button {
		font: inherit;
		text-align: start;
		padding: .65rem .7rem;
		border: 0;
		border-radius: 4px;
		background: transparent;
		color: var(--text-muted);
		cursor: pointer;
	}
	nav button:hover, nav button.active { background: var(--control); color: var(--text); }
	nav button:focus-visible { outline: 2px solid #127cff; }

	:global(#settings-sidebar) {
		width: 12rem;
		flex-shrink: 0;
		background-color: var(--bg-panel);
		border-inline-end: 1px solid var(--border);
	}
	:global(#settings > div) {
		display: flex;
		flex-direction: row;
		width: 56rem;
		height: 42rem;
	}
	#content {
		flex: 1;
		min-width: 0;
		min-height: 0;
		overflow: auto;
		display: flex;
		flex-direction: column;
		padding: 1.5rem;
	}
	#open-count {
		min-height: 100%;
		display: flex;
		flex-direction: column;
	}
	#open-count > div:first-child {
		flex: 1;
	}
	#open-count > div:last-child {
		margin-top: 1rem;
		text-align: center;
		font-size: .8em;
		padding-bottom: 1.5rem;
		margin-bottom: -1.5rem;
	}
</style>
