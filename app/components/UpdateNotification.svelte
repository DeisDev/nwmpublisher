<script>
	import CloudDownload from '@lucide/svelte/icons/cloud-download';
	import { _ } from 'svelte-i18n';
	import tippy from 'tippy.js';

	import { getUpdateVersion } from '../update-check.js';

	const updateAvailable = getUpdateVersion(AppData.version);

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
{/await}

<style>
	:global(.update-popper) {
		text-align: center !important;
	}
	:global(#update-icon) {
		animation: update-available 1.5s infinite alternate;
		opacity: 1 !important;
	}

	@keyframes update-available {
		0% {
			color: #9effc9;
		}
		100% {
			color: white;
		}
	}
</style>
