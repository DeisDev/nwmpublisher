import { invoke } from '@tauri-apps/api/core';
import { get, writable } from 'svelte/store';

export const collections = writable({ loading: false, loaded: false, error: null, items: [] });
export const collectionPicker = writable(null);

let loadRequest = 0;

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

export function addToCollections(ids) {
	collectionPicker.set({ items: [...new Set(ids.map(String))] });
}

export function workshopUrl(id) {
	return 'https://steamcommunity.com/sharedfiles/filedetails/?id=' + id;
}

export function compareTitles(left, right) {
	return left.localeCompare(right, undefined, { numeric: true, sensitivity: 'base' });
}
