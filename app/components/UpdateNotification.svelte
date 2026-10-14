<script>
	import CloudAlert from '@lucide/svelte/icons/cloud-alert';
	import CloudDownload from '@lucide/svelte/icons/cloud-download';
	import { _ } from 'svelte-i18n';
	import tippy from 'tippy.js';

	import { getUpdateVersion } from '../update-check.js';

	let updateAvailable = checkForUpdate();

	function checkForUpdate(options) {
		return getUpdateVersion(AppData.version, options).catch(error => {
			console.warn('Could not check for updates', error);
			throw error;
		});
	}

	function retry() {
		updateAvailable = checkForUpdate({ retry: false });
	}

	function failedTooltip(node) {
		const instance = tippy(node, {
			content: $_('update_check_failed'),
			interactive: false,
		});
		return {
			destroy() {
				instance.destroy();
			}
		};
	}

	function tooltip(node, version) {
		const instance = tippy(node, {
			content: $_('update_available', { values: { version } }),
			interactive: false,
			showOnCreate: true,
			trigger: 'manual',
		});
		instance.popper.classList.add('update-popper');

		const timeout = setTimeout(() => {
			instance.hide();
			instance.setProps({
				trigger: 'mouseenter'
			});
		}, 10000);

		return {
			destroy() {
				clearTimeout(timeout);
				instance.destroy();
			}
		};
	}
</script>

{#await updateAvailable} {:then newVersion}
	{#if newVersion}
		<a href="https://github.com/DeisDev/nwmpublisher/releases/tag/{encodeURIComponent(newVersion)}" target="_blank" use:tooltip={newVersion} class="nav-icon">
			<CloudDownload class="icon" size="1.5rem" stroke-width="1.5" id="update-icon"/>
		</a>
	{/if}
{:catch}
	<button type="button" class="nav-icon update-check-failed" on:click={retry} use:failedTooltip aria-label={$_('update_check_failed')}>
		<CloudAlert class="icon" size="1.5rem" stroke-width="1.5"/>
	</button>
{/await}

<style>
	.update-check-failed {
		display: flex;
		padding: 0;
		border: none;
		background: none;
		color: inherit;
		cursor: pointer;
	}
	:global(.update-popper) {
		text-align: center !important;
	}
	:global(#update-icon) {
		animation: update-available 1.5s infinite alternate;
		opacity: 1 !important;
	}

	@keyframes update-available {
		0% {
			color: var(--text-success);
		}
		100% {
			color: var(--text);
		}
	}
</style>
