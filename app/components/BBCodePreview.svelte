<script context="module">
	const elements = { b: 'strong', i: 'em', u: 'u', strike: 's', h1: 'h1', h2: 'h2', h3: 'h3', table: 'table', tr: 'tr', th: 'th', td: 'td' };
</script>

<script>
	import { _ } from 'svelte-i18n';
	import BBCodeImage from './BBCodeImage.svelte';
	export let nodes;
</script>

{#each nodes as node}
	{#if typeof node === 'string'}
		{node}
	{:else if node.tag === 'img'}
		<BBCodeImage source={node.text}/>
	{:else if node.tag === 'code'}
		<pre><code>{node.text}</code></pre>
	{:else if node.tag === 'noparse'}
		{node.text}
	{:else if node.tag === 'hr'}
		<hr/>
	{:else if node.tag === 'url'}
		<span class="link" title={node.argument}><svelte:self nodes={node.children}/></span>
	{:else if node.tag === 'spoiler'}
		<details class="spoiler"><summary>{$_('bbcode.spoiler')}</summary><svelte:self nodes={node.children}/></details>
	{:else if node.tag === 'quote'}
		<blockquote>{#if node.argument}<cite>{node.argument}</cite>{/if}<svelte:self nodes={node.children}/></blockquote>
	{:else if node.tag === 'list' || node.tag === 'olist'}
		<svelte:element this={node.tag === 'list' ? 'ul' : 'ol'} class="list">
			{#each node.children as child}
				{#if typeof child !== 'string' || child.trim()}
					<li><svelte:self nodes={child.tag === 'item' ? child.children : [child]}/></li>
				{/if}
			{/each}
		</svelte:element>
	{:else if elements[node.tag]}
		<svelte:element this={elements[node.tag]} class="formatted" class:noborder={node.noborder} class:equalcells={node.equalcells}><svelte:self nodes={node.children}/></svelte:element>
	{:else}
		<svelte:self nodes={node.children}/>
	{/if}
{/each}

<style>
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
		padding-left: 1.5rem;
		margin: .4rem 0;
		white-space: normal;
	}
	li {
		white-space: pre-wrap;
	}
	table.formatted {
		--cell-border: 1px solid #666;
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
		text-align: left;
		vertical-align: top;
		white-space: pre-wrap;
	}
	th.formatted {
		background: #202020;
	}
	pre {
		padding: .7rem;
		background: #202020;
		border-radius: 4px;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	blockquote {
		margin: .5rem 0;
		border-left: 3px solid #666;
		padding-left: .8rem;
	}
	cite {
		display: block;
		color: #aaa;
	}
	hr {
		border: 0;
		border-top: 1px solid #666;
	}
	.link {
		color: #46b0ff;
		text-decoration: underline;
		text-underline-offset: 3px;
	}
	.spoiler {
		background: #202020;
		padding: .2rem .5rem;
		border-radius: 4px;
	}
	summary {
		cursor: pointer;
		color: #aaa;
	}
	summary:focus-visible {
		outline: 2px solid #127cff;
		outline-offset: 2px;
	}
</style>
