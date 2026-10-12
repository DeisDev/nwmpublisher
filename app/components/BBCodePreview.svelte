<script context="module">
	const elements = { b: 'strong', i: 'em', u: 'u', strike: 's', h1: 'h1', h2: 'h2', h3: 'h3', tr: 'tr', th: 'th', td: 'td' };
</script>

<script>
	import { _ } from 'svelte-i18n';
	import { bbcodeWidget, externalHost } from '../bbcode';
	import BBCodeImage from './BBCodeImage.svelte';
	import BBCodeLink from './BBCodeLink.svelte';
	import BBCodeWidget from './BBCodeWidget.svelte';
	export let nodes;
	export let widgets = false;
	export let sourceMap = false;
	export let steam = false;
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
			{@const host = steam ? externalHost(node.href) : null}
			<BBCodeLink href={node.href}><svelte:self nodes={node.children} {sourceMap} {steam}/></BBCodeLink>{#if host}<span class="link-host">{$_('bbcode.link_host', { values: { host } })}</span>{/if}
		{:else}
			<svelte:self nodes={node.children} {sourceMap} {steam}/>
		{/if}
	{:else if node.tag === 'spoiler' && steam}
		<span class="spoiler-bar" title={$_('bbcode.spoiler')} data-source-start={sourceMap ? node.start : undefined}><span><svelte:self nodes={node.children} {widgets} {sourceMap} {steam}/></span></span>
	{:else if node.tag === 'spoiler'}
		<details class="spoiler" data-source-start={sourceMap ? node.start : undefined}><summary>{$_('bbcode.spoiler')}</summary><svelte:self nodes={node.children} {widgets} {sourceMap} {steam}/></details>
	{:else if node.tag === 'quote'}
		{@const [authorBefore, authorAfter] = steam ? $_('bbcode.quote_author', { values: { author: '\u0000' } }).split('\u0000') : []}
		<blockquote data-source-start={sourceMap ? node.start : undefined}>{#if node.argument && steam}<div class="quote-author">{authorBefore}<b>{node.argument}</b>{authorAfter}</div>{:else if node.argument}<cite>{node.argument}</cite>{/if}<svelte:self nodes={node.children} {widgets} {sourceMap} {steam}/></blockquote>
	{:else if node.tag === 'list' || node.tag === 'olist'}
		<svelte:element this={node.tag === 'list' ? 'ul' : 'ol'} class="list" data-source-start={sourceMap ? node.start : undefined}>
			{#each node.children as child}
				{#if child.tag !== 'text' || child.text.trim()}
					<li data-source-start={sourceMap ? child.start : undefined}><svelte:self nodes={child.tag === 'item' ? child.children : [child]} {widgets} {sourceMap} {steam}/></li>
				{/if}
			{/each}
		</svelte:element>
	{:else if node.tag === 'table'}
		<div class="table-scroll" data-source-start={sourceMap ? node.start : undefined}><table class="formatted" class:noborder={node.noborder} class:equalcells={node.equalcells}><tbody><svelte:self nodes={node.children} {widgets} {sourceMap} {steam}/></tbody></table></div>
	{:else if elements[node.tag]}
		<svelte:element this={elements[node.tag]} data-source-start={sourceMap ? node.start : undefined} class="formatted" class:noborder={node.noborder} class:equalcells={node.equalcells}><svelte:self nodes={node.children} {widgets} {sourceMap} {steam}/></svelte:element>
	{:else}
		<svelte:self nodes={node.children} {widgets} {sourceMap} {steam}/>
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
	:global(.bbcode.steam) h1.formatted, :global(.bbcode.steam) h2.formatted, :global(.bbcode.steam) h3.formatted {
		margin: 8px 0 6px;
		color: var(--bbcode-heading, var(--link));
		font-weight: 400;
	}
	:global(.bbcode.steam) h1.formatted {
		margin: 0 0 10px;
		font-size: 20px;
		line-height: 23px;
	}
	:global(.bbcode.steam) h2.formatted {
		font-size: 18px;
		line-height: 21px;
	}
	:global(.bbcode.steam) h3.formatted {
		font-size: 16px;
		line-height: 19px;
		font-weight: 300;
	}
	:global(.bbcode.steam a) {
		color: var(--link);
		text-decoration: none;
	}
	:global(.bbcode.steam a:hover) {
		color: var(--bbcode-link-hover, var(--link));
	}
	.link-host {
		padding: 4px;
		color: var(--text-muted);
		font-size: 10px;
	}
	:global(.bbcode.steam) blockquote {
		display: table;
		margin: 8px;
		padding: 12px;
		border: 1px solid var(--bbcode-quote-border, var(--border-muted));
		border-radius: 3px;
		font-size: 92%;
	}
	.quote-author {
		font-size: 92%;
		font-style: italic;
	}
	:global(.bbcode.steam) pre {
		margin: 8px;
		padding: 12px;
		border: 1px solid var(--bbcode-code-border, var(--border-muted));
		border-radius: 3px;
		background: none;
		font: 11px Consolas, monospace;
	}
	:global(.bbcode.steam) pre code {
		font-family: inherit;
	}
	:global(.bbcode.steam) table.formatted {
		font-size: 12px;
	}
	:global(.bbcode.steam) th.formatted, :global(.bbcode.steam) td.formatted {
		padding: 4px;
		background: none;
	}
	:global(.bbcode.steam) td.formatted {
		vertical-align: middle;
	}
	.spoiler-bar {
		padding: 0 8px;
		background: #000;
		color: #000;
	}
	.spoiler-bar > span {
		visibility: hidden;
	}
	.spoiler-bar:hover, .spoiler-bar:focus-within, :global(:focus-visible) .spoiler-bar {
		color: #fff;
	}
	.spoiler-bar:hover > span, .spoiler-bar:focus-within > span, :global(:focus-visible) .spoiler-bar > span {
		visibility: visible;
	}
</style>
