import { Channel, invoke } from '@tauri-apps/api/core';
import { derived, get, writable } from 'svelte/store';
import { saveSettings, settings } from './settings.js';

export const collections = writable({ loading: false, loaded: false, error: null, items: [] });
export const collectionPicker = writable(null);
export const subscriptionSnapshot = writable({ loading: false, progress: null, error: null, data: null });
export const mirrors = derived(settings, $settings => new Set($settings.collections_mirrors ?? []));
export const includeExtras = derived(settings, $settings => !!$settings.collections_sync_include_extras);

const SNAPSHOT_MAX_AGE = 5 * 60;
let loadRequest = 0;
let snapshotRequest = 0;

export async function loadCollections() {
	const request = ++loadRequest;
	collections.update(state => ({ ...state, loading: true, error: null }));
	try {
		const items = await invoke('my_collections');
		if (request === loadRequest) collections.set({ loading: false, loaded: true, error: null, items });
	} catch (error) {
		if (request === loadRequest) collections.update(state => ({ ...state, loading: false, error: String(error) }));
	}
}

export function ensureCollections() {
	const state = get(collections);
	if (!state.loaded && !state.loading) loadCollections();
}

export async function loadSnapshot(refresh = false) {
	const request = ++snapshotRequest;
	subscriptionSnapshot.update(state => ({ ...state, loading: true, progress: null, error: null }));
	const onProgress = new Channel(progress => {
		if (request === snapshotRequest) subscriptionSnapshot.update(state => ({ ...state, progress }));
	});
	try {
		const data = await invoke('subscription_snapshot', { refresh, onProgress });
		if (request === snapshotRequest) subscriptionSnapshot.set({ loading: false, progress: null, error: null, data });
	} catch (error) {
		if (request === snapshotRequest) subscriptionSnapshot.update(state => ({ ...state, loading: false, progress: null, error: String(error) }));
	}
}

export function ensureSnapshot() {
	const state = get(subscriptionSnapshot);
	if (state.loading) return;
	if (!state.data || Date.now() / 1000 - state.data.fetchedAt > SNAPSHOT_MAX_AGE) loadSnapshot();
}

export function setMirror(id, mirrored) {
	const next = new Set(get(mirrors));
	if (mirrored) next.add(id);
	else next.delete(id);
	return saveSettings({ collections_mirrors: [...next] });
}

export function setIncludeExtras(value) {
	return saveSettings({ collections_sync_include_extras: value });
}

/** Keep in step with `plan_sync` in src-tauri/src/steam/subscriptions.rs. */
export function planSync(collection, items, entries, subscriptions, extras) {
	const compared = entry => extras || entry.kind === 'addon';
	const subscribed = new Map(subscriptions.map(entry => [entry.id, entry]));
	const members = new Set(items);
	const plan = { add: [], remove: [], both: [], skipped: { unsupported: 0, extras: 0, unavailable: 0 } };
	for (const item of items) {
		const subscription = subscribed.get(item);
		const entry = subscription ?? entries.get(item);
		if (!entry) continue;
		if (entry.status === 'unsupported') plan.skipped.unsupported++;
		else if (entry.status !== 'unavailable' && !compared(entry)) plan.skipped.extras++;
		else if (subscription) plan.both.push(item);
		else plan.remove.push(item);
	}
	for (const entry of subscriptions) {
		if (members.has(entry.id)) continue;
		if (entry.id === collection || entry.status === 'unsupported') plan.skipped.unsupported++;
		else if (entry.status !== 'available') plan.skipped.unavailable++;
		else if (!compared(entry)) plan.skipped.extras++;
		else plan.add.push(entry.id);
	}
	return plan;
}

export function addToCollections(ids) {
	collectionPicker.set({ items: [...new Set(ids.map(String))] });
}

export function workshopUrl(id) {
	return 'https://steamcommunity.com/sharedfiles/filedetails/?id=' + id;
}

export function compareTitles(left, right) {
	return left.localeCompare(right, undefined, { numeric: true, sensitivity: 'base' });
}
