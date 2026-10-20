import { Channel, invoke } from '@tauri-apps/api/core';
import { message } from '@tauri-apps/plugin-dialog';
import { get, writable } from 'svelte/store';
import { _ } from 'svelte-i18n';
import { settings } from './settings.js';
import { taskMessage } from './transactions.js';
import { playSound } from './sounds.js';

const PROBLEMS_SHOWN = 10;

export const deleteRequest = writable(null);
export const removalJob = writable(null);
export const removedPaths = writable([]);

export function addonName(workshop, gma) {
	return (workshop && !workshop.dead && workshop.title) || gma?.title || gma?.extractedName || gma?.path;
}

export function addonRemovals(paths) {
	return invoke('installed_addon_removal', { paths });
}

export async function addonRemoval(path) {
	return (await addonRemovals([path]))[0];
}

export function stopRemoval() {
	return invoke('stop_installed_addon_removal');
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

let job = 0;
async function runRemoval(unsubscribe, paths, options) {
	if (get(removalJob)) throw { key: 'addon_removal_busy' };
	const current = ++job;
	removalJob.set({ unsubscribe, completed: 0, total: paths.length });
	const onProgress = new Channel(progress => {
		if (current === job) removalJob.update(state => state && { ...state, ...progress });
	});
	try {
		return await invoke(unsubscribe ? 'unsubscribe_installed_addons' : 'delete_installed_addons', { paths, ...options, onProgress });
	} finally {
		if (current === job) removalJob.set(null);
	}
}

export async function unsubscribeAddon(path, name, deleteFiles, permanent) {
	let outcome;
	try {
		[outcome] = await runRemoval(true, [path], { deleteFiles, permanent });
	} catch (error) {
		return failed('addon_unsubscribe_failed', name, error);
	}
	if (outcome.error) return failed('addon_unsubscribe_failed', name, outcome.error);
	const result = outcome.files;
	removedPaths.set([path]);
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
	let outcome;
	try {
		[outcome] = await runRemoval(false, [path], { permanent });
	} catch (error) {
		return failed('addon_delete_failed', name, error);
	}
	if (outcome.error) return failed('addon_delete_failed', name, outcome.error);
	removedPaths.set([path]);
	playSound('delete');
	taskMessage(get(_)(permanent ? 'addon_deleted' : 'addon_trashed', { values: { name } }));
	return true;
}

export async function removeAddons(items, unsubscribe, deleteFiles, permanent) {
	let outcomes;
	try {
		outcomes = await runRemoval(unsubscribe, items.map(item => item.path), unsubscribe ? { deleteFiles, permanent } : { permanent });
	} catch (error) {
		playSound('error');
		await message(get(_)('bulk_removal_error', { values: { error: errorText(error) } }), { kind: 'error' });
		return null;
	}
	const names = new Map(items.map(item => [item.path, item.name]));
	const removed = outcomes.filter(outcome => !outcome.error);
	if (removed.length) {
		removedPaths.set(removed.map(outcome => outcome.path));
		const filesRemoved = removed.some(outcome => outcome.files?.removed.length);
		const key = unsubscribe ? (filesRemoved ? (permanent ? 'bulk_unsubscribed_deleted' : 'bulk_unsubscribed_trashed') : 'bulk_unsubscribed') : (permanent ? 'bulk_deleted' : 'bulk_trashed');
		taskMessage(get(_)(key, { values: { count: removed.length } }));
	}
	if (outcomes.length < items.length) {
		taskMessage(get(_)('bulk_removal_stopped', { values: { count: items.length - outcomes.length } }));
	}
	const problems = [];
	let failedAddons = 0;
	for (const outcome of outcomes) {
		const name = names.get(outcome.path);
		const lines = outcome.error ? [`${name}: ${errorText(outcome.error)}`] : (outcome.files?.failed ?? []).map(failure => `${name}: ${failure.path}: ${errorText(failure.error)}`);
		if (lines.length) failedAddons++;
		problems.push(...lines);
	}
	if (problems.length) {
		playSound('error');
		const shown = problems.slice(0, PROBLEMS_SHOWN);
		if (problems.length > PROBLEMS_SHOWN) shown.push(get(_)('bulk_removal_more', { values: { count: problems.length - PROBLEMS_SHOWN } }));
		await message(get(_)('bulk_removal_failed', { values: { count: failedAddons, errors: shown.join('\n') } }), { kind: 'warning' });
	} else if (removed.length) {
		playSound(unsubscribe ? 'success' : 'delete');
	}
	return outcomes;
}

export async function requestUnsubscribe(path, name, onDone) {
	const deleteFiles = !!get(settings).unsubscribe_delete_files;
	const mode = savedDeleteMode();
	if (deleteFiles && !mode) {
		deleteRequest.set({ items: [{ path, name }], unsubscribe: true, deleteFiles, onDone });
		return;
	}
	const result = await unsubscribeAddon(path, name, deleteFiles, mode === 'permanent');
	if (result) onDone?.(result);
}

export async function requestDelete(path, name, onDone) {
	const mode = savedDeleteMode();
	if (!mode) {
		deleteRequest.set({ items: [{ path, name }], unsubscribe: false, deleteFiles: true, onDone });
	} else if (await deleteAddon(path, name, mode === 'permanent')) {
		onDone?.();
	}
}

/** Always asks first, even for one addon, because it is reached by a single key press or a bulk action. */
export function confirmRemoval(items, unsubscribe, onDone) {
	deleteRequest.set({ items, unsubscribe, deleteFiles: !unsubscribe || !!get(settings).unsubscribe_delete_files, onDone });
}
