import { invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import { get, writable } from 'svelte/store';
import { _ } from 'svelte-i18n';
import { settings } from './settings.js';
import { taskMessage } from './transactions.js';
import { playSound } from './sounds.js';

export const deleteRequest = writable(null);

export function addonName(workshop, gma) {
	return (workshop && !workshop.dead && workshop.title) || gma?.title || gma?.extractedName || gma?.path;
}

export function addonRemoval(path) {
	return invoke('installed_addon_removal', { path });
}

function errorText(error) {
	return error?.key ? get(_)(error.key, { values: { detail: error.detail } }) : String(error);
}

async function failed(key, name, error) {
	playSound('error');
	await message(get(_)(key, { values: { name, error: errorText(error) } }), { kind: 'error' });
	return null;
}

function savedDeleteMode() {
	const mode = get(settings).addon_delete_mode;
	return mode === 'trash' || mode === 'permanent' ? mode : null;
}

export async function unsubscribeAddon(path, name, deleteFiles, permanent) {
	let result;
	try {
		result = await invoke('unsubscribe_installed_addon', { path, deleteFiles, permanent });
	} catch (error) {
		return failed('addon_unsubscribe_failed', name, error);
	}
	const removed = result.removed.length > 0;
	taskMessage(get(_)(removed ? (permanent ? 'addon_unsubscribed_deleted' : 'addon_unsubscribed_trashed') : 'addon_unsubscribed', { values: { name } }));
	if (result.failed.length) {
		playSound('error');
		const errors = result.failed.map(failure => `${failure.path}: ${errorText(failure.error)}`).join('\n');
		await message(get(_)('addon_unsubscribe_partial', { values: { name, count: result.failed.length, errors } }), { kind: 'warning' });
	} else {
		playSound('success');
	}
	return result;
}

export async function deleteAddon(path, name, permanent) {
	try {
		await invoke('delete_installed_addon', { path, permanent });
	} catch (error) {
		return failed('addon_delete_failed', name, error);
	}
	playSound('delete');
	taskMessage(get(_)(permanent ? 'addon_deleted' : 'addon_trashed', { values: { name } }));
	return true;
}

export async function requestUnsubscribe(path, name, onDone) {
	const deleteFiles = !!get(settings).unsubscribe_delete_files;
	const mode = savedDeleteMode();
	if (deleteFiles && !mode) {
		deleteRequest.set({ path, name, onDone, unsubscribe: true });
		return;
	}
	const result = await unsubscribeAddon(path, name, deleteFiles, mode === 'permanent');
	if (result) onDone?.(result);
}

export async function requestDelete(path, name, onDone) {
	const mode = savedDeleteMode();
	if (!mode) {
		deleteRequest.set({ path, name, onDone, unsubscribe: false });
	} else if (await deleteAddon(path, name, mode === 'permanent')) {
		onDone?.();
	}
}
