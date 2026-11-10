import { invoke } from '@tauri-apps/api/core';
import { get, writable } from 'svelte/store';
import { _ } from 'svelte-i18n';
import { taskError } from './transactions.js';

export const settings = writable({});
export const settingsSave = writable({ state: 'saved', error: null });
let queue = Promise.resolve();
let pending = 0;
let batchError = null;

function saveFailed(error) {
	settingsSave.set({ state: 'failed', error });
	taskError(get(_)('settings_save_failed') + ': ' + error);
}

export function saveSettings(patch) {
	const values = structuredClone(patch);
	if (pending === 0) batchError = null;
	pending++;
	settingsSave.set({ state: 'saving', error: null });
	const operation = queue.then(() => invoke('update_settings', { patch: values }));
	queue = operation.catch(() => {});
	return operation.then(result => {
		if (--pending === 0) settingsSave.set({ state: batchError ? 'failed' : 'saved', error: batchError });
		return result;
	}, error => {
		pending--;
		batchError = String(error);
		saveFailed(batchError);
		throw error;
	});
}

window.addEventListener('settings-save-error', event => saveFailed(String(event.detail)));
