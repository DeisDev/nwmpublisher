<script>
	import { _ } from 'svelte-i18n';
	import { ChevronRight } from 'akar-icons-svelte';
	import { createEventDispatcher, onDestroy } from 'svelte';
	import { parseBBCode } from '../bbcode';
	import BBCodePreview from './BBCodePreview.svelte';

	export let id;
	export let label;
	export let value = '';
	export let disabled = false;
	export let error = null;
	export let help = '';
	export let size = '';
	export let formattingOpen = false;
	export let active = true;

	const dispatch = createEventDispatcher();
	const formats = [
		{ tag: 'b', label: 'B', key: 'b' }, { tag: 'i', label: 'I', key: 'i' }, { tag: 'u', label: 'U', key: 'u' },
		{ tag: 'strike', label: 'S', key: 'x', shift: true },
		{ tag: 'h1', label: 'H1', key: '1', shift: true }, { tag: 'h2', label: 'H2', key: '2', shift: true }, { tag: 'h3', label: 'H3', key: '3', shift: true },
		{ tag: 'spoiler', key: 's', shift: true }, { tag: 'url', key: 'k' }, { tag: 'img', key: 'm', shift: true },
		{ tag: 'list', key: '8', shift: true }, { tag: 'olist', key: '7', shift: true }, { tag: 'code', key: 'e', shift: true },
		{ tag: 'quote', key: 'q', shift: true }, { tag: 'noparse', key: 'l', shift: true }, { tag: 'hr', label: 'HR', key: 'h', shift: true },
		{ tag: 'table', key: '4', shift: true }, { tag: 'tr', key: '5', shift: true }, { tag: 'th', key: '6', shift: true }, { tag: 'td', key: '9', shift: true },
	];
	let input;
	let host;
	let editor;
	let fullscreenDialog;
	let fullscreenButton;
	let fullscreen = false;
	$: nodes = parseBBCode(value);
	$: describedBy = [help && `${id}-help`, size && `${id}-size`, error && `${id}-error`].filter(Boolean).join(' ') || undefined;
	$: if (!active && fullscreen) closeFullscreen(false);

	onDestroy(() => {
		if (fullscreenDialog?.open) fullscreenDialog.close();
	});

	function toggleFullscreen() {
		if (fullscreen) {
			closeFullscreen();
			return;
		}
		if (!active) return;
		const selection = [input.selectionStart, input.selectionEnd, input.selectionDirection];
		const scrollTop = input.scrollTop;
		// Keep the same textarea when entering or leaving fullscreen.
		fullscreenDialog.append(editor);
		fullscreenDialog.showModal();
		fullscreen = true;
		(disabled ? fullscreenButton : input).focus({ preventScroll: true });
		input.setSelectionRange(...selection);
		input.scrollTop = scrollTop;
	}

	function closeFullscreen(restoreFocus = true) {
		const selection = [input.selectionStart, input.selectionEnd, input.selectionDirection];
		const scrollTop = input.scrollTop;
		fullscreenDialog.close();
		host.append(editor);
		fullscreen = false;
		if (restoreFocus) fullscreenButton.focus({ preventScroll: true });
		input.setSelectionRange(...selection);
		input.scrollTop = scrollTop;
	}

	function shortcut(format) {
		return (format.shift ? 'Shift+' : '') + format.key.toUpperCase();
	}

	function onKeydown(event) {
		if (disabled || event.isComposing || event.keyCode === 229 || event.altKey || !(event.ctrlKey || event.metaKey)) return;
		const key = /^Digit\d$/.test(event.code) ? event.code.slice(-1) : event.key.toLowerCase();
		const format = formats.find(format => format.key === key && !!format.shift === event.shiftKey);
		if (!format) return;
		event.preventDefault();
		event.stopPropagation();
		if (!event.repeat) formatText(format.tag);
	}

	function changed() {
		value = input.value;
		dispatch('input', value);
	}

	function formatText(tag) {
		if (disabled) return;
		const start = input.selectionStart;
		let end = input.selectionEnd;
		let text = input.value.slice(start, end) || (tag === 'img' ? 'https://' : $_(tag === 'url' ? 'bbcode.link_text' : 'bbcode.text'));
		let opening = tag === 'url' ? '[url=https://]' : `[${tag}]`;
		let closing = `[/${tag}]`;
		if (tag === 'hr') {
			opening = (start > 0 && input.value[start - 1] !== '\n' ? '\n' : '') + '[hr][/hr]\n';
			text = closing = '';
			end = start;
		} else if (tag === 'quote') {
			opening = `[quote=${$_('bbcode.author')}]`;
		} else if (tag === 'table') {
			opening = `[table]\n[tr][th]${$_('bbcode.header')} 1[/th][th]${$_('bbcode.header')} 2[/th][/tr]\n[tr][td]`;
			closing = `[/td][td]${$_('bbcode.text')}[/td][/tr]\n[/table]`;
		} else if (tag === 'tr') {
			opening = '[tr][td]';
			closing = '[/td][/tr]';
		} else if (tag === 'list' || tag === 'olist') {
			opening += '\n';
			closing = '\n' + closing;
			text = text.split('\n').map(line => '[*]' + line).join('\n');
		}
		input.setRangeText(opening + text + closing, start, end, 'select');
		changed();
		input.focus();
		if (tag === 'url' || tag === 'quote') input.setSelectionRange(start + tag.length + 2, start + opening.length - 1);
		else input.setSelectionRange(start + opening.length, start + opening.length + text.length);
	}
</script>

<div class="editor-host" bind:this={host}>
	<dialog id={`${id}-fullscreen`} bind:this={fullscreenDialog} aria-label={label} on:cancel|preventDefault|stopPropagation={() => closeFullscreen()}></dialog>
	<div class="editor" bind:this={editor}>
		<div class="editor-heading">
			<div class="editor-title">
				{#if fullscreen}
					<button type="button" class="back-button" title={$_('bbcode.restore')} on:click={() => closeFullscreen()}>
						<span aria-hidden="true">←</span>{$_('bbcode.back')}
					</button>
				{/if}
				<label for={id}>{label}</label>
			</div>
			<div class="editor-actions">
				<button type="button" class="formatting-toggle" aria-expanded={formattingOpen} aria-controls={`${id}-toolbar`} on:click={() => formattingOpen = !formattingOpen}>
					<span class="chevron" class:expanded={formattingOpen}><ChevronRight size=".85rem"/></span>{$_('bbcode.formatting')}
				</button>
				<button type="button" class="fullscreen-toggle" bind:this={fullscreenButton} title={$_(fullscreen ? 'bbcode.restore' : 'bbcode.fullscreen')} aria-label={$_(fullscreen ? 'bbcode.restore' : 'bbcode.fullscreen')} aria-pressed={fullscreen} aria-controls={`${id}-fullscreen`} on:click={toggleFullscreen}>
					<svg width="1rem" height="1rem" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
						{#if fullscreen}<path d="M9 3v6H3m12-6v6h6M9 21v-6H3m12 6v-6h6"/>{:else}<path d="M9 3H3v6m12-6h6v6M3 15v6h6m6 0h6v-6"/>{/if}
					</svg>
				</button>
			</div>
		</div>
		<div id={`${id}-toolbar`} class="bbcode-toolbar" hidden={!formattingOpen} role="group" aria-label={$_('bbcode.toolbar')} aria-controls={id}>
			{#each formats as format}
				<button type="button" data-format={format.tag} title={`${$_('bbcode.' + format.tag)} (Ctrl/Cmd+${shortcut(format)})`} aria-label={$_('bbcode.' + format.tag)} aria-keyshortcuts={`Control+${shortcut(format)} Meta+${shortcut(format)}`} {disabled} on:click={() => formatText(format.tag)}>{format.label ?? $_('bbcode.' + format.tag)}</button>
			{/each}
		</div>
		<textarea {id} bind:this={input} value={value} on:input={changed} on:keydown={onKeydown} {disabled} class:error={error !== null} aria-invalid={error !== null} aria-describedby={describedBy}></textarea>
		{#if help}<p id={`${id}-help`}>{help}</p>{/if}
		{#if size}<p id={`${id}-size`}>{size}</p>{/if}
		{#if error}<p id={`${id}-error`} class="error-message" role="alert">{$_(error)}</p>{/if}
		<div class="preview-heading"><span id={`${id}-preview-label`}>{$_('bbcode.preview')}</span><span class="live">{$_('bbcode.live')}</span></div>
		<!-- Keep the scrollable preview focusable for keyboard scrolling. -->
		<div class="preview select" role="region" aria-labelledby={`${id}-preview-label`} tabindex="0">
			{#if value}<BBCodePreview {nodes}/>{:else}<span class="empty">{$_('bbcode.preview_empty')}</span>{/if}
		</div>
	</div>
</div>

<style>
	.editor-host {
		display: flex;
		flex: 1;
		min-height: 0;
		min-width: 0;
	}
	dialog[open] {
		display: flex;
		position: fixed;
		inset: 0;
		width: 100%;
		height: 100%;
		max-width: none;
		max-height: none;
		margin: 0;
		padding: 1rem;
		border: 0;
		background: #1a1a1a;
		color: inherit;
		font: inherit;
	}
	.editor {
		display: flex;
		flex-direction: column;
		gap: .5rem;
		flex: 1;
		min-height: 0;
		min-width: 0;
	}
	.editor-heading, .preview-heading {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: .75rem;
	}
	.editor-actions {
		display: flex;
		gap: .4rem;
		flex-shrink: 0;
	}
	.editor-title {
		display: flex;
		align-items: center;
		gap: .75rem;
		min-width: 0;
	}
	.back-button {
		display: flex;
		align-items: center;
		gap: .35rem;
		flex-shrink: 0;
	}
	.fullscreen-toggle {
		display: flex;
		align-items: center;
		justify-content: center;
	}
	button {
		font: inherit;
		font-size: .8em;
		padding: .35rem .5rem;
		border: 1px solid #414141;
		border-radius: 4px;
		background: #313131;
		color: #fff;
		cursor: pointer;
	}
	button:hover:not(:disabled) {
		background: #414141;
	}
	button:focus-visible, .preview:focus-visible {
		outline: 2px solid #127cff;
		outline-offset: 1px;
	}
	button:disabled {
		opacity: .5;
		cursor: default;
	}
	.formatting-toggle {
		display: flex;
		align-items: center;
		gap: .3rem;
		flex-shrink: 0;
	}
	.chevron {
		display: inline-flex;
		transition: transform .15s;
	}
	.chevron.expanded {
		transform: rotate(90deg);
	}
	.bbcode-toolbar {
		display: flex;
		flex-wrap: wrap;
		gap: .3rem;
	}
	.bbcode-toolbar[hidden] {
		display: none;
	}
	.bbcode-toolbar button {
		min-width: 2rem;
	}
	[data-format='b'] {
		font-weight: bold;
	}
	[data-format='i'] {
		font-style: italic;
	}
	[data-format='u'] {
		text-decoration: underline;
	}
	[data-format='strike'] {
		text-decoration: line-through;
	}
	textarea {
		appearance: none;
		font: .85em/1.5 monospace;
		border-radius: 4px;
		border: none;
		background: rgba(255,255,255,.1);
		box-shadow: 0 0 2px rgb(0 0 0 / 40%);
		padding: .7rem;
		color: #fff;
		width: 100%;
		resize: none;
		flex: 1 1 0;
		min-height: 7rem;
	}
	textarea:focus {
		box-shadow: inset 0 0 0 1.5px #127cff;
		outline: none;
	}
	textarea.error {
		box-shadow: inset 0 0 0 1.5px var(--error);
	}
	p {
		margin: 0;
		font-size: .8em;
		line-height: 1.5;
		text-align: center;
	}
	.error-message {
		color: var(--error);
	}
	.preview-heading {
		border-top: 1px solid #414141;
		padding-top: .65rem;
		margin-top: .25rem;
	}
	.live, .empty {
		color: #aaa;
		font-size: .85em;
	}
	.preview {
		flex: 1 1 0;
		min-height: 7rem;
		overflow: auto;
		padding: .8rem;
		border-radius: 4px;
		background: #292929;
		font-size: .9em;
		line-height: 1.5;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	.preview :global(*) {
		user-select: text;
	}
	@media (prefers-reduced-motion: reduce) {
		.chevron {
			transition: none;
		}
	}
</style>
