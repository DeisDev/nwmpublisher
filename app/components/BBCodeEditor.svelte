<script>
	import { _ } from 'svelte-i18n';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import FileInput from '@lucide/svelte/icons/file-input';
	import { createEventDispatcher, onDestroy } from 'svelte';
	import { closingTag, linkText, listBreak, parseBBCode, unwrapTag } from '../bbcode';
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
	export let historyKey = 0;
	export let convertPaste = null;
	export let importLabel = '';
	export let importDisabled = false;

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
	let undoStack = [];
	let redoStack = [];
	let current = { value, selection: [0, 0, 'none'], scrollTop: 0, scrollLeft: 0 };
	let previousHistoryKey = historyKey;
	let beforeInput = null;
	let composition = null;
	let lastEdit = null;
	let listItem = null;
	$: syncHistory(value, historyKey);
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
		if (disabled || composition || event.isComposing || event.keyCode === 229) return;
		if (['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown'].includes(event.key)) lastEdit = null;
		listItem = null;
		if (event.key === 'Enter' && !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey) {
			breakLine(event);
			return;
		}
		if (event.altKey || !(event.ctrlKey || event.metaKey)) return;
		const key = /^Digit\d$/.test(event.code) ? event.code.slice(-1) : event.key.toLowerCase();
		if (key === 'z' || (key === 'y' && event.ctrlKey && !event.shiftKey)) {
			event.preventDefault();
			event.stopPropagation();
			restoreHistory(key === 'y' || event.shiftKey);
			return;
		}
		const format = formats.find(format => format.key === key && !!format.shift === event.shiftKey);
		if (!format) return;
		event.preventDefault();
		event.stopPropagation();
		if (!event.repeat) formatText(format.tag);
	}

	function breakLine(event) {
		if (input.selectionStart !== input.selectionEnd) return;
		const result = listBreak(input.value, input.selectionStart);
		if (!result) return;
		// Let the browser insert the line break so it keeps the caret in view, then add the marker.
		if (result.marker) {
			listItem = result;
			return;
		}
		event.preventDefault();
		const before = snapshot();
		input.setRangeText(result.text, result.start, result.end, 'end');
		recordChange(before);
	}

	function insertListItem() {
		const caret = input.selectionStart;
		input.setRangeText(listItem.marker + listItem.suffix, caret, caret, 'start');
		input.setSelectionRange(caret + listItem.marker.length, caret + listItem.marker.length);
	}

	function typeBracket(text) {
		const start = input.selectionStart;
		const end = input.selectionEnd;
		let replacement;
		if (text === '[' && start !== end && AppSettings.bbcode_wrap_selection) {
			replacement = `[${input.value.slice(start, end)}]`;
		} else if (text === ']' && start === end && AppSettings.bbcode_auto_close_tags) {
			const closing = closingTag(input.value, start);
			if (!closing) return false;
			replacement = ']' + closing;
		} else {
			return false;
		}
		const before = snapshot();
		input.setRangeText(replacement, start, end, 'start');
		input.setSelectionRange(start + 1, text === '[' ? end + 1 : start + 1);
		recordChange(before, text === ']' ? 'insertText' : undefined);
		return true;
	}

	function onPaste(event) {
		if (disabled || composition) return;
		const start = input.selectionStart;
		const end = input.selectionEnd;
		const text = (event.clipboardData?.getData('text/plain') ?? '').replace(/\r\n?/g, '\n');
		const converted = convertPaste?.(text);
		if (converted != null) {
			event.preventDefault();
			// Record the original paste first so a single undo restores it.
			const before = snapshot();
			input.setRangeText(text, start, end, 'end');
			recordChange(before, 'insertFromPaste');
			const pasted = snapshot();
			input.setRangeText(converted, start, start + text.length, 'end');
			recordChange(pasted);
			return;
		}
		const selected = input.value.slice(start, end);
		const link = linkText(text);
		if (!selected.trim() || !link || linkText(selected)) return;
		event.preventDefault();
		const before = snapshot();
		input.setRangeText(`[url=${link}]${selected}[/url]`, start, end, 'end');
		recordChange(before, 'insertFromPaste');
	}

	export function insert(text) {
		if (disabled || composition) return;
		const before = snapshot();
		input.focus({ preventScroll: true });
		input.setRangeText(text, before.selection[0], before.selection[1], 'end');
		recordChange(before);
	}

	function changed() {
		current = snapshot();
		value = current.value;
		dispatch('input', value);
	}

	function snapshot() {
		return {
			value: input.value,
			selection: [input.selectionStart, input.selectionEnd, input.selectionDirection],
			scrollTop: input.scrollTop,
			scrollLeft: input.scrollLeft,
		};
	}

	function syncHistory(nextValue, nextKey) {
		if (nextValue === current.value && nextKey === previousHistoryKey) return;
		// Loading another addon starts fresh, even when its text matches this one.
		current = { value: nextValue, selection: [0, 0, 'none'], scrollTop: 0, scrollLeft: 0 };
		previousHistoryKey = nextKey;
		undoStack = [];
		redoStack = [];
		beforeInput = composition = lastEdit = null;
	}

	function recordChange(before, inputType) {
		beforeInput = null;
		if (before.value === input.value) return;
		const time = Date.now();
		const groupable = ['insertText', 'deleteContentBackward', 'deleteContentForward'].includes(inputType);
		const continuing = groupable && lastEdit?.type === inputType && time - lastEdit.time < 1000
			&& before.selection[0] === before.selection[1] && before.selection[0] === lastEdit.end;
		if (!continuing) undoStack = [...undoStack, before].slice(-100);
		redoStack = [];
		lastEdit = groupable ? { type: inputType, time, end: input.selectionEnd } : null;
		changed();
	}

	function restoreHistory(redo = false) {
		if (disabled || composition) return;
		lastEdit = beforeInput = null;
		const stack = redo ? redoStack : undoStack;
		if (!stack.length) return;
		const state = stack[stack.length - 1];
		if (redo) {
			undoStack = [...undoStack, snapshot()];
			redoStack = redoStack.slice(0, -1);
		} else {
			redoStack = [...redoStack, snapshot()];
			undoStack = undoStack.slice(0, -1);
		}
		input.value = state.value;
		input.focus({ preventScroll: true });
		input.setSelectionRange(...state.selection);
		input.scrollTop = state.scrollTop;
		input.scrollLeft = state.scrollLeft;
		changed();
	}

	function onBeforeInput(event) {
		if (disabled || composition) return;
		if (event.inputType === 'historyUndo' || event.inputType === 'historyRedo') {
			event.preventDefault();
			restoreHistory(event.inputType === 'historyRedo');
		} else if (event.inputType === 'insertText' && typeBracket(event.data)) {
			event.preventDefault();
		} else {
			beforeInput = snapshot();
		}
	}

	function onInput(event) {
		if (composition) changed();
		else {
			if (listItem && ['insertLineBreak', 'insertParagraph'].includes(event.inputType)) insertListItem();
			recordChange(beforeInput ?? current, event.inputType);
		}
		listItem = null;
	}

	function onCompositionStart() {
		composition = snapshot();
		beforeInput = lastEdit = null;
	}

	function onCompositionEnd() {
		const before = composition;
		composition = null;
		if (before) recordChange(before);
	}

	function formatText(tag) {
		if (disabled || composition) return;
		const before = snapshot();
		const start = input.selectionStart;
		let end = input.selectionEnd;
		const wrapped = unwrapTag(input.value, start, end, tag);
		if (wrapped) {
			input.setRangeText(wrapped.text, wrapped.start, wrapped.end, 'select');
			input.focus();
			recordChange(before);
			return;
		}
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
		input.focus();
		if (tag === 'url' || tag === 'quote') input.setSelectionRange(start + tag.length + 2, start + opening.length - 1);
		else input.setSelectionRange(start + opening.length, start + opening.length + text.length);
		recordChange(before);
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
				<button type="button" title={`${$_('bbcode.undo')} (Ctrl/Cmd+Z)`} aria-label={$_('bbcode.undo')} aria-keyshortcuts="Control+Z Meta+Z" disabled={disabled || !!composition || !undoStack.length} on:click={() => restoreHistory()}>
					<span aria-hidden="true">↶</span>
				</button>
				<button type="button" title={`${$_('bbcode.redo')} (Ctrl+Y / Ctrl/Cmd+Shift+Z)`} aria-label={$_('bbcode.redo')} aria-keyshortcuts="Control+Y Control+Shift+Z Meta+Shift+Z" disabled={disabled || !!composition || !redoStack.length} on:click={() => restoreHistory(true)}>
					<span aria-hidden="true">↷</span>
				</button>
				{#if importLabel}
					<button type="button" class="import-button" disabled={disabled || importDisabled} on:click={() => dispatch('import')}>
						<FileInput class="icon" size=".85rem"/>{importLabel}
					</button>
				{/if}
				<button type="button" class="formatting-toggle" aria-expanded={formattingOpen} aria-controls={`${id}-toolbar`} on:click={() => formattingOpen = !formattingOpen}>
					<span class="chevron" class:expanded={formattingOpen}><ChevronRight class="icon" size=".85rem"/></span>{$_('bbcode.formatting')}
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
		<textarea {id} bind:this={input} value={value} {disabled}
			on:beforeinput={onBeforeInput} on:input={onInput} on:keydown={onKeydown} on:paste={onPaste}
			on:compositionstart={onCompositionStart} on:compositionend={onCompositionEnd}
			on:pointerdown={() => lastEdit = null} on:blur={() => lastEdit = null}
			class:error={error !== null} aria-invalid={error !== null} aria-describedby={describedBy}
		></textarea>
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
		background: var(--bg-app);
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
		overflow: auto;
		padding: 2px;
	}
	.editor-heading, .preview-heading {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: .75rem;
		flex-shrink: 0;
	}
	.editor-heading {
		flex-wrap: wrap;
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
		border: 1px solid var(--border);
		border-radius: 4px;
		background: var(--control);
		color: var(--text);
		cursor: pointer;
	}
	button:hover:not(:disabled) {
		background: var(--control-hover);
	}
	button:focus-visible, .preview:focus-visible {
		outline: 2px solid #127cff;
		outline-offset: 1px;
	}
	button:disabled {
		opacity: .5;
		cursor: default;
	}
	.formatting-toggle, .import-button {
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
		flex-shrink: 0;
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
		background: var(--fill);
		box-shadow: 0 0 2px rgb(0 0 0 / 40%);
		padding: .7rem;
		color: var(--text);
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
		flex-shrink: 0;
		font-size: .8em;
		line-height: 1.5;
		text-align: center;
	}
	.error-message {
		color: var(--error);
	}
	.preview-heading {
		border-top: 1px solid var(--border);
		padding-top: .65rem;
		margin-top: .25rem;
	}
	.live, .empty {
		color: var(--text-muted);
		font-size: .85em;
	}
	.preview {
		flex: 1 1 0;
		min-height: 7rem;
		overflow: auto;
		padding: .8rem;
		border-radius: 4px;
		background: var(--bg-preview);
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
