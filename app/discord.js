import { invoke } from '@tauri-apps/api/core';
import { writable } from 'svelte/store';

export const publisherPresence = writable(null);
export const settingsPresence = writable(false);

const activities = [
	'my_workshop', 'installed_addons', 'downloader', 'size_analyzer', 'addon_cleaner',
	'preparing_addon', 'editing_addon', 'settings', 'publishing', 'packaging',
	'downloading', 'extracting', 'cancelling',
];

export function updateDiscordPresence(screen, translate) {
	const labels = Object.fromEntries(activities.map(activity => [activity, translate('discord.' + activity)]));
	invoke('update_discord_presence', { screen, labels }).catch(error => console.error('Discord presence:', error));
}
