<script>
	import { _ } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import { writeText } from '@tauri-apps/plugin-clipboard-manager';
	import Copy from '@lucide/svelte/icons/copy';
	import Check from '@lucide/svelte/icons/check';
	import FolderOpen from '@lucide/svelte/icons/folder-open';
	import Bug from '@lucide/svelte/icons/bug';
	import MonitorX from '@lucide/svelte/icons/monitor-x';
	import LinkOut from '@lucide/svelte/icons/external-link';
	import X from '@lucide/svelte/icons/x';
	import { translateError } from '../i18n';

	const issues = 'https://github.com/DeisDev/nwmpublisher/issues/new';
	// GitHub limits an issue to 65,536 characters, and the rest of the form needs some of them.
	const longLog = 60000;

	let open = false;
	let root;
	let toggleButton;
	let copying = false;
	let copied = false;
	let long = false;
	let error = null;

	function issueUrl(template) {
		return issues + '?' + new URLSearchParams({ template, version: AppData.version });
	}

	function toggle() {
		open = !open;
		copied = false;
		error = null;
	}

	function close() {
		open = false;
		toggleButton.focus();
	}

	function dismiss(event) {
		if (open && !root.contains(event.target)) open = false;
	}

	function onKeydown(event) {
		if (open && event.key === 'Escape') close();
	}

	// A fence longer than any backtick run in the log keeps it in one code block on GitHub.
	function codeBlock(log) {
		let longest = 2;
		for (const match of log.matchAll(/`+/g)) longest = Math.max(longest, match[0].length);
		const fence = '`'.repeat(longest + 1);
		return `${fence}text\n${log.trimEnd()}\n${fence}`;
	}

	async function copyLog() {
		copying = true;
		copied = false;
		error = null;
		try {
			const text = codeBlock(await invoke('read_current_log'));
			await writeText(text);
			long = text.length > longLog;
			copied = true;
		} catch (err) {
			error = $_('report_copy_failed', { values: { error: translateError(err) } });
		} finally {
			copying = false;
		}
	}

	function openLogs() {
		error = null;
		invoke('open_logs').catch(err => error = translateError(err));
	}
</script>

<svelte:window on:pointerdown={dismiss} on:keydown={onKeydown}/>

<div class="report" bind:this={root}>
	{#if open}
		<div id="report-problem" class="popover" role="dialog" aria-labelledby="report-problem-title">
			<header>
				<h2 id="report-problem-title">{$_('report_problem')}</h2>
				<button type="button" class="close" aria-label={$_('close')} on:click={close}><X size="1rem"/></button>
			</header>
			<ol>
				<li>
					<span class="step">{$_('report_step_log')}</span>
					<div class="row">
						<button type="button" class="action primary" disabled={copying} on:click={copyLog} aria-live="polite">
							{#if copied}<Check size="1rem"/>{$_('report_log_copied')}{:else}<Copy size="1rem"/>{$_('report_copy_log')}{/if}
						</button>
						<button type="button" class="action" on:click={openLogs}><FolderOpen size="1rem"/>{$_('settings.paths.open_logs')}</button>
					</div>
					{#if copied && long}<p class="note">{$_('report_log_long')}</p>{/if}
					{#if error}<p class="note error" role="alert">{error}</p>{/if}
				</li>
				<li>
					<span class="step">{$_('report_step_issue')}</span>
					<a class="action link" href={issueUrl('1-bug.yml')} target="_blank"><Bug size="1rem"/><span>{$_('report_bug')}</span><LinkOut size=".8rem"/></a>
					<a class="action link" href={issueUrl('2-crash-or-display.yml')} target="_blank"><MonitorX size="1rem"/><span>{$_('report_crash')}</span><LinkOut size=".8rem"/></a>
				</li>
			</ol>
		</div>
	{/if}
	<button type="button" class="toggle" aria-expanded={open} aria-controls="report-problem" bind:this={toggleButton} on:click={toggle}>{$_('report_problem')}</button>
</div>

<style>
	.report {
		position: relative;
		display: flex;
		justify-content: center;
	}
	.toggle {
		padding: 0;
		border: 0;
		background: none;
		color: var(--text-faint);
		font: inherit;
		line-height: inherit;
		text-decoration: underline;
		text-underline-offset: 5px;
		cursor: pointer;
		transition: color .25s;
	}
	.toggle:hover, .toggle[aria-expanded='true'] {
		color: var(--text);
	}
	.popover {
		position: absolute;
		inset-block-end: calc(100% + .5rem);
		inset-inline-start: 0;
		width: min(19rem, calc(100vw - 2rem));
		padding: .9rem;
		border: 1px solid var(--border-strong);
		border-radius: 4px;
		background: var(--bg-raised);
		box-shadow: 0 6px 18px #0008;
		color: var(--text);
		font-size: .85rem;
		line-height: 1.5;
		text-align: start;
	}
	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: .5rem;
	}
	h2 {
		margin: 0;
		font-size: .95rem;
	}
	ol {
		display: flex;
		flex-direction: column;
		gap: .75rem;
		margin: .5rem 0 0;
		padding-inline-start: 1.1rem;
	}
	li {
		display: flex;
		flex-direction: column;
		gap: .4rem;
	}
	.step {
		color: var(--text-soft);
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: .4rem;
	}
	.action {
		display: inline-flex;
		align-items: center;
		gap: .45rem;
		padding: .5rem .65rem;
		border: 0;
		border-radius: 4px;
		background: var(--control);
		color: var(--text);
		font: inherit;
		text-decoration: none;
		cursor: pointer;
	}
	.action:hover:not(:disabled) {
		background: var(--control-hover);
	}
	.action.primary {
		background: var(--neutral);
		color: #fff;
	}
	.action.primary:hover:not(:disabled) {
		background: var(--neutral);
		filter: brightness(1.15);
	}
	.action:disabled {
		opacity: .6;
		cursor: default;
	}
	.link span {
		flex: 1;
	}
	.note {
		margin: 0;
		color: var(--text-muted);
		font-size: .8rem;
	}
	.error {
		color: var(--text);
		border-inline-start: 3px solid var(--error);
		padding-inline-start: .5rem;
		overflow-wrap: anywhere;
	}
	.close {
		display: inline-flex;
		padding: .25rem;
		border: 0;
		border-radius: 4px;
		background: none;
		color: var(--text-muted);
		cursor: pointer;
	}
	.close:hover {
		background: var(--control);
		color: var(--text);
	}
	button:focus-visible, a:focus-visible {
		outline: 2px solid #127cff;
		outline-offset: 2px;
	}
</style>
