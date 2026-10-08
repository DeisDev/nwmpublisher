<script context="module">
	const elements = { b: 'strong', i: 'em', u: 'u', strike: 's', h1: 'h1', h2: 'h2', h3: 'h3', tr: 'tr', th: 'th', td: 'td' };
</script>

<script>
	import { _ } from 'svelte-i18n';
	import { bbcodeWidget } from '../bbcode';
	import BBCodeImage from './BBCodeImage.svelte';
	import BBCodeLink from './BBCodeLink.svelte';
	import BBCodeWidget from './BBCodeWidget.svelte';
	export let nodes;
	export let widgets = false;
	export let sourceMap = false;
</script>

{#each nodes as node}
	{#if node.tag === 'text'}
		{#if sourceMap}<span data-source-start={node.start} data-source-text>{node.text}</span>{:else}{node.text}{/if}
	{:else if node.tag === 'img'}
		<BBCodeImage source={node.text} sourceStart={sourceMap ? node.start : undefined}/>
	{:else if node.tag === 'code'}
		<pre data-source-start={sourceMap ? node.start : undefined}><code data-source-start={sourceMap ? node.contentStart : undefined} data-source-text={sourceMap ? '' : undefined}>{node.text}</code></pre>
	{:else if node.tag === 'noparse'}
		{#if sourceMap}<span data-source-start={node.contentStart} data-source-text>{node.text}</span>{:else}{node.text}{/if}
	{:else if node.tag === 'hr'}
		<hr data-source-start={sourceMap ? node.start : undefined}/>
	{:else if node.tag === 'url'}
		{@const widget = widgets && node.automatic && node.href ? bbcodeWidget(node.href) : null}
		{#if widget}
			<BBCodeWidget {widget} href={node.href} sourceStart={sourceMap ? node.start : undefined}/>
		{:else if node.href}
			<BBCodeLink href={node.href}><svelte:self nodes={node.children} {sourceMap}/></BBCodeLink>
		{:else}
			<svelte:self nodes={node.children} {sourceMap}/>
		{/if}
	{:else if node.tag === 'spoiler'}
		<details class="spoiler" data-source-start={sourceMap ? node.start : undefined}><summary>{$_('bbcode.spoiler')}</summary><svelte:self nodes={node.children} {widgets} {sourceMap}/></details>
	{:else if node.tag === 'quote'}
		<blockquote data-source-start={sourceMap ? node.start : undefined}>{#if node.argument}<cite>{node.argument}</cite>{/if}<svelte:self nodes={node.children} {widgets} {sourceMap}/></blockquote>
	{:else if node.tag === 'list' || node.tag === 'olist'}
		<svelte:element this={node.tag === 'list' ? 'ul' : 'ol'} class="list" data-source-start={sourceMap ? node.start : undefined}>
			{#each node.children as child}
				{#if child.tag !== 'text' || child.text.trim()}
					<li data-source-start={sourceMap ? child.start : undefined}><svelte:self nodes={child.tag === 'item' ? child.children : [child]} {widgets} {sourceMap}/></li>
				{/if}
			{/each}
		</svelte:element>
	{:else if node.tag === 'table'}
		<div class="table-scroll" data-source-start={sourceMap ? node.start : undefined}><table class="formatted" class:noborder={node.noborder} class:equalcells={node.equalcells}><tbody><svelte:self nodes={node.children} {widgets} {sourceMap}/></tbody></table></div>
	{:else if elements[node.tag]}
		<svelte:element this={elements[node.tag]} data-source-start={sourceMap ? node.start : undefined} class="formatted" class:noborder={node.noborder} class:equalcells={node.equalcells}><svelte:self nodes={node.children} {widgets} {sourceMap}/></svelte:element>
	{:else}
		<svelte:self nodes={node.children} {widgets} {sourceMap}/>
	{/if}
{/each}

<style>
	span[data-source-text] { display: contents; }
	.formatted {
		overflow-wrap: anywhere;
	}
	h1.formatted, h2.formatted, h3.formatted {
		margin: .4rem 0;
		line-height: 1.3;
	}
	h1.formatted {
		font-size: 1.6em;
	}
	h2.formatted {
		font-size: 1.35em;
	}
	h3.formatted {
		font-size: 1.15em;
	}
	.list {
		padding-inline-start: 1.5rem;
		margin: .4rem 0;
		white-space: normal;
	}
	li {
		white-space: pre-wrap;
	}
	.table-scroll {
		max-width: 100%;
		overflow-x: auto;
	}
	table.formatted {
		--cell-border: 1px solid var(--border-muted);
		border-collapse: collapse;
		max-width: 100%;
		margin: .5rem 0;
		white-space: normal;
	}
	table.formatted.noborder {
		--cell-border: 0;
	}
	table.formatted.equalcells {
		width: 100%;
		table-layout: fixed;
	}
	th.formatted, td.formatted {
		border: var(--cell-border);
		padding: .4rem .6rem;
		text-align: start;
		vertical-align: top;
		white-space: pre-wrap;
	}
	th.formatted {
		background: var(--bg-subtle);
	}
	pre {
		padding: .7rem;
		background: var(--bg-subtle);
		border-radius: 4px;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	blockquote {
		margin: .5rem 0;
		border-inline-start: 3px solid var(--border-muted);
		padding-inline-start: .8rem;
	}
	cite {
		display: block;
		color: var(--text-muted);
	}
	hr {
		border: 0;
		border-top: 1px solid var(--border-muted);
	}
	.spoiler {
		background: var(--bg-subtle);
		padding: .2rem .5rem;
		border-radius: 4px;
	}
	summary {
		cursor: pointer;
		color: var(--text-muted);
	}
	summary:focus-visible {
		outline: 2px solid #127cff;
		outline-offset: 2px;
	}
</style>
