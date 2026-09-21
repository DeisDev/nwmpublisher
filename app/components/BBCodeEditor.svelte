<script>
	import { _ } from 'svelte-i18n';
	import { ChevronRight } from 'akar-icons-svelte';
	import { createEventDispatcher } from 'svelte';
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

	const dispatch = createEventDispatcher();
	const formats = [
		{ tag: 'b', label: 'B' }, { tag: 'i', label: 'I' }, { tag: 'u', label: 'U' }, { tag: 'strike', label: 'S' },
		{ tag: 'h1', label: 'H1' }, { tag: 'h2', label: 'H2' }, { tag: 'h3', label: 'H3' },
		{ tag: 'spoiler' }, { tag: 'url' }, { tag: 'img' }, { tag: 'list' }, { tag: 'olist' }, { tag: 'code' },
	];
	let input;
	$: nodes = parseBBCode(value);
	$: describedBy = [help && `${id}-help`, size && `${id}-size`, error && `${id}-error`].filter(Boolean).join(' ') || undefined;

	function changed() {
		value = input.value;
		dispatch('input', value);
	}

	function formatText(tag) {
		if (disabled) return;
		const start = input.selectionStart;
		const end = input.selectionEnd;
		let text = input.value.slice(start, end) || (tag === 'img' ? 'https://' : $_(tag === 'url' ? 'bbcode.link_text' : 'bbcode.text'));
		let opening = tag === 'url' ? '[url=https://]' : `[${tag}]`;
		let closing = `[/${tag}]`;
		if (tag === 'list' || tag === 'olist') {
			opening += '\n';
			closing = '\n' + closing;
			text = text.split('\n').map(line => '[*]' + line).join('\n');
		}
		input.setRangeText(opening + text + closing, start, end, 'select');
		changed();
		input.focus();
		if (tag === 'url') input.setSelectionRange(start + '[url='.length, start + opening.length - 1);
		else input.setSelectionRange(start + opening.length, start + opening.length + text.length);
	}
</script>

<div class="editor">
	<div class="editor-heading">
		<label for={id}>{label}</label>
		<button type="button" class="formatting-toggle" aria-expanded={formattingOpen} aria-controls={`${id}-toolbar`} on:click={() => formattingOpen = !formattingOpen}>
			<span class="chevron" class:expanded={formattingOpen}><ChevronRight size=".85rem"/></span>{$_('bbcode.formatting')}
		</button>
	</div>
	<div id={`${id}-toolbar`} class="bbcode-toolbar" hidden={!formattingOpen} role="group" aria-label={$_('bbcode.toolbar')} aria-controls={id}>
		{#each formats as format}
			<button type="button" data-format={format.tag} title={$_('bbcode.' + format.tag)} aria-label={$_('bbcode.' + format.tag)} {disabled} on:click={() => formatText(format.tag)}>{format.label ?? $_('bbcode.' + format.tag)}</button>
		{/each}
	</div>
	<textarea {id} bind:this={input} value={value} on:input={changed} {disabled} class:error={error !== null} aria-invalid={error !== null} aria-describedby={describedBy}></textarea>
	{#if help}<p id={`${id}-help`}>{help}</p>{/if}
	{#if size}<p id={`${id}-size`}>{size}</p>{/if}
	{#if error}<p id={`${id}-error`} class="error-message" role="alert">{$_(error)}</p>{/if}
	<div class="preview-heading"><span id={`${id}-preview-label`}>{$_('bbcode.preview')}</span><span class="live">{$_('bbcode.live')}</span></div>
	<!-- Keep the scrollable preview focusable for keyboard scrolling. -->
	<div class="preview select" role="region" aria-labelledby={`${id}-preview-label`} tabindex="0">
		{#if value}<BBCodePreview {nodes}/>{:else}<span class="empty">{$_('bbcode.preview_empty')}</span>{/if}
	</div>
</div>

<style>
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
