<script>
	export let view = 'grid';
	export let count = 12;
	export let template = '';
</script>

{#each { length: count } as _, index (index)}
	{#if view === 'list'}
		<div class="placeholder row" style:grid-template-columns={template} inert aria-hidden="true"><span></span><span class="title"><span class="image"></span><span class="line"></span></span></div>
	{:else}
		<div class="placeholder card" inert aria-hidden="true"><span class="line short"></span><span class="image"></span><span class="line"></span></div>
	{/if}
{/each}

<style>
	.placeholder {
		animation: pulse 1.4s ease-in-out infinite;
	}
	.card {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: .8rem;
		align-self: start;
		padding: .8rem;
		border: 1px solid var(--border);
		border-radius: 4px;
	}
	.image {
		background: var(--bg-sunken-2);
	}
	.card .image {
		width: 100%;
		aspect-ratio: 1;
	}
	.line {
		width: 70%;
		height: .9rem;
		border-radius: 3px;
		background: var(--control);
	}
	.line.short {
		width: 40%;
		align-self: flex-start;
	}
	.row {
		display: grid;
		align-items: center;
		gap: .75rem;
		padding: .4rem .75rem;
		border-bottom: 1px solid var(--border);
	}
	.title {
		display: flex;
		align-items: center;
		gap: .6rem;
	}
	.row .image {
		flex-shrink: 0;
		width: 2rem;
		height: 2rem;
		border-radius: 3px;
	}
	.row .line {
		width: 40%;
	}
	@keyframes pulse {
		50% {
			opacity: .45;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.placeholder {
			animation: none;
		}
	}
</style>
