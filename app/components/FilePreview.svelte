<script>
	import { _, locale } from 'svelte-i18n';
	import { onDestroy } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import X from '@lucide/svelte/icons/x';
	import { translateError } from '../i18n';
	import { highlightLua } from '../lua';
	import Loading from './Loading.svelte';
	import SyntaxText from './SyntaxText.svelte';

	export let gmaPath = null;
	export let contentPath = null;
	export let entryPath;
	export let close;
	export let openEntry;

	let preview = null;
	let error = null;
	let url = null;
	let request = 0;
	$: load(gmaPath, contentPath, entryPath);
	$: isText = preview && ['lua', 'text'].includes(preview.kind);
	$: source = isText ? preview.data.replace(/\r\n?/g, '\n') : '';
	$: tokens = preview?.kind === 'lua' ? highlightLua(source) : [{ text: source, kind: '' }];
	$: numbers = new Intl.NumberFormat($locale, { useGrouping: false });
	$: lineNumbers = source.split('\n').map((_, index) => numbers.format(index + 1)).join('\n');

	function release() {
		if (url) URL.revokeObjectURL(url);
		url = null;
	}

	async function load(gmaPath, contentPath, entryPath) {
		const current = ++request;
		release();
		preview = error = null;
		try {
			const result = contentPath
				? await invoke('preview_folder_entry', { contentPath, entryPath })
				: await invoke('preview_gma_entry', { gmaPath, entryPath });
			if (current !== request) return;
			if (!['lua', 'text'].includes(result.kind)) {
				const binary = atob(result.data);
				const bytes = new Uint8Array(binary.length);
				for (let index = 0; index < binary.length; index++) bytes[index] = binary.charCodeAt(index);
				url = URL.createObjectURL(new Blob([bytes], { type: result.mime }));
			}
			preview = url ? { kind: result.kind } : result;
		} catch (cause) {
			if (current === request) error = translateError(cause);
		}
	}

	onDestroy(() => {
		request++;
		release();
	});
</script>

<section class="file-preview" aria-label={$_('file_preview.title')}>
	<header>
		<div class="file-title"><strong class="select">{entryPath}</strong>{#if isText}<span>{$_('file_preview.read_only')}</span>{/if}</div>
		<button type="button" on:click={() => openEntry(entryPath)}>{$_(contentPath ? 'file_preview.open_externally' : 'extract')}</button>
		<button type="button" aria-label={$_('file_preview.close')} title={$_('file_preview.close')} on:click={close}><X class="icon" size="1rem"/></button>
	</header>
	{#if error}
		<p class="error select" role="alert">{error}</p>
	{:else if !preview}
		<div class="loading"><Loading size="2rem"/></div>
	{:else if isText}
		<div class="code-view" tabindex="0" role="textbox" aria-readonly="true" aria-multiline="true" aria-label={$_('file_preview.read_only')}>
			<pre class="line-numbers" aria-hidden="true">{lineNumbers}</pre>
			<pre class="source select"><code><SyntaxText {tokens}/>{source.endsWith('\n') ? '\n' : ''}</code></pre>
		</div>
	{:else}
		<div class="media">
			{#if preview.kind === 'image'}
				<img src={url} alt={entryPath} on:error={() => error = $_('file_preview.media_error')}/>
			{:else if preview.kind === 'audio'}
				<audio src={url} controls preload="metadata" aria-label={entryPath} on:error={() => error = $_('file_preview.media_error')}></audio>
			{:else if preview.kind === 'video'}
				<video src={url} controls preload="metadata" aria-label={entryPath} on:error={() => error = $_('file_preview.media_error')}><track kind="captions"/></video>
			{/if}
		</div>
	{/if}
</section>

<style>
	.file-preview { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; border-inline-start: 1px solid var(--border); background: var(--bg-content); }
	header { display: flex; align-items: center; gap: .5rem; padding: .7rem; background: var(--bg-panel); border-bottom: 1px solid var(--border); }
	.file-title { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: .25rem; }
	strong { font-size: .85rem; overflow-wrap: anywhere; }
	.file-title span { color: var(--text-muted); font-size: .75rem; }
	button { display: flex; align-items: center; flex-shrink: 0; font: inherit; font-size: .8rem; border: 1px solid var(--border); border-radius: 4px; padding: .4rem .5rem; color: var(--text); background: var(--control); cursor: pointer; }
	button:hover { background: var(--control-hover); }
	button:focus-visible, .code-view:focus-visible { outline: 2px solid #127cff; outline-offset: -2px; }
	.loading, .media { display: flex; align-items: center; justify-content: center; flex: 1; min-height: 0; min-width: 0; padding: 1rem; overflow: auto; }
	.media img, .media video { max-width: 100%; max-height: 100%; object-fit: contain; }
	audio { width: 100%; max-width: 30rem; }
	.error { white-space: pre-wrap; overflow-wrap: anywhere; padding: 1rem; margin: 0; overflow: auto; color: var(--text-error); }
	.code-view { display: flex; flex: 1; min-height: 0; overflow: auto; }
	pre { margin: 0; padding: 1rem; font: .85rem/1.6 monospace; font-variant-ligatures: none; tab-size: 4; white-space: pre; overflow-wrap: normal; }
	code { font: inherit; }
	.source { flex: 1; }
	.source :global(*) { user-select: text; }
	.line-numbers { position: sticky; inset-inline-start: 0; align-self: flex-start; color: var(--text-muted); background: var(--bg-panel); text-align: end; border-inline-end: 1px solid var(--border); }
	@media (max-width: 900px) { .file-preview { border-inline-start: 0; border-top: 1px solid var(--border); } }
</style>
