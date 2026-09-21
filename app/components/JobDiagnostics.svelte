<script>
	import { writeText } from '@tauri-apps/api/clipboard';
	import { message } from '@tauri-apps/api/dialog';
	import { _ } from 'svelte-i18n';

	export let transaction;
	export let context = {};
	let copied = false;

	async function copy() {
		copied = false;
		const diagnostics = JSON.stringify({
			jobId: transaction.id,
			status: transaction.status,
			progress: transaction.progress,
			finished: transaction.finished,
			cancelled: transaction.cancelled,
			error: transaction.error,
			...context
		}, null, 2);
		try {
			await writeText(diagnostics);
			copied = true;
		} catch (error) {
			await message($_('copy_diagnostics_failed', { values: { error: String(error) } }), { type: 'error' });
		}
	}
</script>

<button type="button" on:click|stopPropagation={copy} aria-live="polite">{copied ? $_('diagnostics_copied') : $_('copy_diagnostics')}</button>

<style>
	button {
		font: inherit;
		color: inherit;
		background: rgba(0, 0, 0, .2);
		border: 1px solid rgba(255, 255, 255, .4);
		border-radius: .25rem;
		cursor: pointer;
		padding: .25rem .5rem;
		white-space: nowrap;
	}
</style>
