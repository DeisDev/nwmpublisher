import { invoke } from '@tauri-apps/api/core';
import { writable } from 'svelte/store';

export const settingsSave = writable({ state: 'saved', error: null });
let queue = Promise.resolve();
let pending = 0;
let batchError = null;

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
		settingsSave.set({ state: 'failed', error: batchError });
		throw error;
	});
}

window.addEventListener('settings-save-error', event => settingsSave.set({ state: 'failed', error: String(event.detail) }));
