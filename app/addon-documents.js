import { invoke } from '@tauri-apps/api/core';
import * as dialog from '@tauri-apps/plugin-dialog';
import { get } from 'svelte/store';
import { _ } from 'svelte-i18n';

export async function readAddonDocument(contentPath, document, file) {
	const translate = get(_);
	try {
		const text = await invoke('read_addon_document', { contentPath, document });
		if (text === null) await dialog.message(translate('addon_document.missing', { values: { file } }), { kind: 'warning' });
		return text;
	} catch (error) {
		const message = String(error) === 'ERR_ADDON_DOCUMENT_TOO_LARGE'
			? translate('ERR_ADDON_DOCUMENT_TOO_LARGE', { values: { file } })
			: translate('addon_document.failed', { values: { file, error: String(error) } });
		await dialog.message(message, { kind: 'error' });
		return null;
	}
}
