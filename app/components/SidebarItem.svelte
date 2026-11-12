<script>
	import { _ } from 'svelte-i18n';

	export let id;
	export let activeItem;
	export let click = null;
	export let jobs = null;
	export let count = null;
</script>

<item class:active={$activeItem == id} on:click={() => { $activeItem = id; if (click) click(id) }}>
	<span><slot></slot>{#if count && $count != null}<small>{$_('nav_count', { values: { count: $count } })}</small>{/if}</span>
	{#if jobs}
		<div class="jobs" class:active={$jobs > 0}>{$jobs}</div>
	{/if}
</item>

<style>
	item {
		position: relative;
		display: flex;
		padding: .5rem;
		padding-left: .7rem;
		padding-right: .7rem;
		cursor: pointer;
		border-radius: 4px;
		align-items: center;
		height: calc(1.181em + .5rem + .5rem);
		max-height: calc(1.181em + .5rem + .5rem);
		color: var(--text-soft);
		transition: background-color .12s, color .12s;
	}
	item:hover {
		background-color: var(--fill);
		color: var(--text);
	}
	item > span {
		flex: 1;
	}
	small {
		margin-inline-start: .4em;
		color: var(--text-muted);
		font-size: .85em;
	}
	item > .jobs {
		padding: .25em .4em;
		background-color: #007eff;
		color: #fff;
		border-radius: .35rem;
		font-size: .75em;
		text-align: center;
		white-space: nowrap;
		vertical-align: baseline;
		margin-right: -.4em;

		transform: scale(0);
		opacity: 0;
		transition: transform .1s, opacity .1s;
	}
	item > .jobs.active {
		transform: scale(1);
		opacity: 1;
	}
	item.active {
		background-color: var(--bg-hover);
		color: var(--text);
	}
	item.active::before {
		content: '';
		position: absolute;
		inset-block: .45rem;
		inset-inline-start: 0;
		width: 3px;
		border-radius: 3px;
		background: var(--neutral);
	}
	item:not(:last-child) {
		margin-bottom: .5rem;
	}
</style>
