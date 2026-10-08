<script>
	export let tokens = [];
</script>

{#each tokens as token}<span class={token.kind ? 'syntax-' + token.kind : undefined}>{token.text}</span>{/each}

<style>
	span { user-select: inherit; }
	.syntax-tag, .syntax-keyword, .syntax-function { color: var(--syntax-keyword); }
	.syntax-string { color: var(--syntax-string); }
	.syntax-number { color: var(--syntax-number); }
	.syntax-comment { color: var(--syntax-comment); }
	.syntax-operator { color: var(--syntax-operator); }
</style>
