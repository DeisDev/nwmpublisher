import { Channel, invoke } from '@tauri-apps/api/core';
import { get, writable } from 'svelte/store';
import { _ } from 'svelte-i18n';
import { taskError, taskMessage } from './transactions.js';
import { playSound } from './sounds.js';
import { addonName, addonRemovals, confirmRemoval } from './addon-removal.js';
import { bbcodeWidget } from './bbcode.js';

export const KINDS = ['addon', 'collection', 'dupe', 'save', 'demo'];
export const SOURCES = ['everyone', 'author', 'friends', 'followed', 'favorites'];
export const TREND_DAYS = [1, 7, 30, 90, 180, 365];
export const ADDON_TYPES = ['gamemode', 'map', 'weapon', 'vehicle', 'npc', 'entity', 'tool', 'effects', 'model', 'ServerContent'];
export const ADDON_TAGS = ['fun', 'roleplay', 'scenic', 'movie', 'realism', 'cartoon', 'water', 'comic', 'build'];
export const COLUMNS = ['author', 'type', 'tags', 'size', 'subscribers', 'favorites', 'rating', 'created', 'updated', 'status'];
export const KIND_TAGS = new Set(['addon', 'dupe', 'save', 'demo']);

const SORTS = {
	everyone: ['popular', 'top_rated', 'most_subscribed', 'newest', 'updated', 'relevance'],
	author: ['newest', 'updated', 'top_rated'],
	favorites: ['newest', 'updated', 'top_rated'],
	friends: ['newest'],
	followed: ['newest'],
};
const SUPERSEDED = 'A newer search replaced this one.';
const MIB = 1024 * 1024;
const DAY = 24 * 60 * 60;
const AUTHORS_PER_REQUEST = 100;
const EMPTY_SCANS_BEFORE_PAUSE = 3;

const t = (key, values) => get(_)(key, { values });

export function sortsFor(source, text) {
	return SORTS[source].filter(sort => sort !== 'relevance' || text.trim());
}

export function defaultQuery() {
	return {
		kind: 'addon',
		source: 'everyone',
		author: null,
		sort: 'popular',
		trendDays: 7,
		text: '',
		types: [],
		tags: [],
		matchAll: false,
		excludedTags: [],
		created: { start: '', end: '' },
		updated: { start: '', end: '' },
		sizeMin: '',
		sizeMax: '',
		minSubscribers: '',
		minRating: '',
		status: 'any',
	};
}

/** Fixes a query after the mode, source, or text changes so that Steam accepts it. */
export function normalizeQuery(query) {
	const sorts = sortsFor(query.source, query.text);
	const next = { ...query };
	if (!sorts.includes(next.sort)) next.sort = sorts[0];
	if (next.source !== 'author') next.author = null;
	if (next.kind !== 'addon') next.types = [];
	if (next.kind === 'collection') {
		next.tags = [];
		next.excludedTags = [];
		next.sizeMin = '';
		next.sizeMax = '';
	}
	return next;
}

function number(value) {
	if (value === '' || value == null) return null;
	const parsed = Number(value);
	return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
}

function dateStart(value) {
	return value ? Math.floor(new Date(value + 'T00:00:00').getTime() / 1000) : null;
}

function range({ start, end }) {
	if (!start && !end) return null;
	const from = dateStart(start);
	const to = dateStart(end);
	return { start: from, end: to == null ? null : to + DAY - 1 };
}

export function requestFor(query) {
	const request = {
		kind: query.kind,
		source: query.source,
		sort: query.sort,
		trendDays: query.trendDays,
		text: query.text.trim(),
		status: query.status,
		created: range(query.created),
		updated: range(query.updated),
		minSubscribers: number(query.minSubscribers),
		minRating: number(query.minRating) == null ? null : Math.min(number(query.minRating), 100) / 100,
	};
	if (query.source === 'author') request.author = query.author?.id;
	if (query.kind !== 'collection') {
		request.tags = query.tags;
		request.matchAll = query.matchAll;
		request.excludedTags = query.excludedTags;
		const sizeMin = number(query.sizeMin);
		const sizeMax = number(query.sizeMax);
		request.sizeMin = sizeMin == null ? null : Math.round(sizeMin * MIB);
		request.sizeMax = sizeMax == null ? null : Math.round(sizeMax * MIB);
	}
	if (query.kind === 'addon') request.types = query.types;
	return request;
}

/** Chips for the filters that differ from the defaults, each with the change that clears it. */
export function activeFilters(query) {
	const chips = [];
	const without = (list, value) => list.filter(item => item !== value);
	for (const type of query.types) chips.push({ key: 'type:' + type, label: t('addon_types.' + type), clear: { types: without(query.types, type) } });
	for (const tag of query.tags) chips.push({ key: 'tag:' + tag, label: tagLabel(tag), clear: { tags: without(query.tags, tag) } });
	for (const tag of query.excludedTags) chips.push({ key: 'not:' + tag, label: t('browse_chip_excluded', { tag: tagLabel(tag) }), clear: { excludedTags: without(query.excludedTags, tag) } });
	if (query.tags.length > 1 && query.matchAll) chips.push({ key: 'all', label: t('browse_match_all'), clear: { matchAll: false } });
	for (const field of ['created', 'updated']) {
		const { start, end } = query[field];
		if (start || end) chips.push({ key: field, label: t('browse_chip_' + field, { start: start || '…', end: end || '…' }), clear: { [field]: { start: '', end: '' } } });
	}
	if (number(query.sizeMin) != null) chips.push({ key: 'sizeMin', label: t('browse_chip_size_min', { size: query.sizeMin }), clear: { sizeMin: '' } });
	if (number(query.sizeMax) != null) chips.push({ key: 'sizeMax', label: t('browse_chip_size_max', { size: query.sizeMax }), clear: { sizeMax: '' } });
	if (number(query.minSubscribers) != null) chips.push({ key: 'subscribers', label: t('browse_chip_subscribers', { count: number(query.minSubscribers) }), clear: { minSubscribers: '' } });
	if (number(query.minRating) != null) chips.push({ key: 'rating', label: t('browse_chip_rating', { rating: number(query.minRating) }), clear: { minRating: '' } });
	if (query.status !== 'any') chips.push({ key: 'status', label: t('browse_status_' + query.status), clear: { status: 'any' } });
	if (query.source !== 'everyone') {
		const label = query.source === 'author' ? t('browse_by',{ name: query.author?.name ?? query.author?.id }) : t('browse_source_' + query.source);
		chips.push({ key: 'source', label, clear: { source: 'everyone', author: null } });
	}
	return chips;
}

export function tagLabel(tag) {
	const same = value => value.toLowerCase() === tag.toLowerCase();
	const type = ADDON_TYPES.find(same);
	if (type) return t('addon_types.' + type);
	const known = ADDON_TAGS.find(same);
	return known ? t('addon_tags.' + known) : tag;
}

// The backend keeps the newest request number, so numbers must keep growing after a page reload.
let requestCounter = Date.now();

/**
 * Pages through Steam with the cursor each page returns; `onPage` receives each page's scan counts.
 * After several scans in a row find nothing, it waits for `resume()` so strict filters do not scan the whole Workshop.
 */
export function browsePager(query, { onPage, onPause } = {}) {
	const request = requestFor(query);
	const id = ++requestCounter;
	const cursors = [null];
	let emptyScans = 0;
	let fetched = 0;
	let resume = null;
	const next = async page => {
		if (page > 1 && cursors[page - 1] == null) return [0, [], false];
		if (page > fetched && emptyScans >= EMPTY_SCANS_BEFORE_PAUSE) {
			await new Promise(resolve => {
				resume = resolve;
				onPause?.(true);
			});
			onPause?.(false);
			emptyScans = 0;
		}
		const result = await invoke('browse_workshop', { query: request, cursor: cursors[page - 1], request: id });
		cursors[page] = result.nextCursor;
		fetched = Math.max(fetched, page);
		emptyScans = result.items.length ? 0 : emptyScans + 1;
		onPage?.(page, result);
		noteInstalled(result.items.map(item => String(item.workshop.id)));
		for (const item of result.items) requestAuthor(item.workshop.steamid64);
		return [result.items.length, result.items, result.more];
	};
	next.resume = () => {
		resume?.();
		resume = null;
	};
	return next;
}

export function isSuperseded(error) {
	return String(error) === SUPERSEDED;
}

export function itemKind(workshop) {
	return workshop.kind ?? 'other';
}

export const subscribedIds = writable(new Set());
export const installedIds = writable(new Set());
export const favoriteIds = writable(null);
export const authors = writable(new Map());
export const browseJob = writable(null);

export async function loadSubscribed() {
	try {
		subscribedIds.set(new Set(await invoke('local_subscribed_ids')));
	} catch (error) {
		console.error('Subscribed items:', error);
	}
}

async function noteInstalled(ids) {
	if (!ids.length) return;
	try {
		const found = new Set((await invoke('installed_addons_by_id', { ids })).map(addon => String(addon.installed.id)));
		installedIds.update(known => {
			const next = new Set(known);
			for (const id of ids) found.has(id) ? next.add(id) : next.delete(id);
			return next;
		});
	} catch (error) {
		console.error('Installed items:', error);
	}
}

export async function loadFavorites() {
	if (get(favoriteIds)) return;
	try {
		favoriteIds.set(new Set(await invoke('my_favorite_ids')));
	} catch (error) {
		console.error('Favorites:', error);
	}
}

let pendingAuthors = new Set();
let authorTimer = null;

export function requestAuthor(id) {
	if (!id || get(authors).has(id) || pendingAuthors.has(id)) return;
	pendingAuthors.add(id);
	authorTimer ??= setTimeout(loadAuthors, 50);
}

async function loadAuthors() {
	authorTimer = null;
	const ids = [...pendingAuthors].slice(0, AUTHORS_PER_REQUEST);
	ids.forEach(id => pendingAuthors.delete(id));
	if (pendingAuthors.size) authorTimer = setTimeout(loadAuthors, 50);
	let users = [];
	try {
		users = await invoke('get_steam_users', { ids });
	} catch (error) {
		console.error('Steam users:', error);
	}
	const found = new Map(users.map(user => [user.steamid, user]));
	authors.update(known => {
		const next = new Map(known);
		for (const id of ids) next.set(id, found.get(id) ?? null);
		return next;
	});
}

function failureLines(failures) {
	return failures.map(failure => `${failure.id}: ${failure.error}`).join('\n');
}

async function runJob(action, total, command, args, stoppable = true) {
	if (get(browseJob)) {
		taskError(t('browse_job_busy'));
		return null;
	}
	browseJob.set({ action, completed: 0, total, retryIn: null, stoppable });
	const onProgress = new Channel(progress => browseJob.update(job => job && { ...job, ...progress }));
	try {
		return await invoke(command, { ...args, onProgress });
	} finally {
		browseJob.set(null);
	}
}

export function stopJob() {
	return invoke('stop_collection_change');
}

function reportOutcome(outcome, key, count) {
	if (count) {
		taskMessage(t(key, { count }));
		playSound('success');
	}
	if (outcome.stopped) taskMessage(t('browse_job_stopped'));
	if (outcome.error || outcome.failures.length) {
		playSound('error');
		taskError([outcome.error, failureLines(outcome.failures)].filter(Boolean).join('\n'));
	}
}

export async function subscribeItems(ids) {
	let outcome;
	try {
		outcome = await runJob('subscribe', ids.length, 'subscribe_workshop_items', { ids: ids.map(String) });
	} catch (error) {
		playSound('error');
		taskError(t('browse_subscribe_failed', { error: String(error) }));
		return;
	}
	if (!outcome) return;
	reportOutcome(outcome, 'browse_subscribed', outcome.subscribed);
	if (outcome.alreadySubscribed) taskMessage(t('browse_already_subscribed', { count: outcome.alreadySubscribed }));
	if (outcome.skipped) taskMessage(t('browse_skipped', { count: outcome.skipped }));
	await loadSubscribed();
}

export async function subscribeCollection(id, size) {
	let outcome;
	try {
		outcome = await runJob('subscribe', size, 'subscribe_collection', { collectionId: String(id) }, false);
	} catch (error) {
		playSound('error');
		taskError(t('browse_subscribe_failed', { error: String(error) }));
		return;
	}
	if (!outcome) return;
	reportOutcome(outcome, 'browse_subscribed', outcome.subscribed);
	if (outcome.alreadySubscribed) taskMessage(t('browse_already_subscribed', { count: outcome.alreadySubscribed }));
	if (outcome.skipped) taskMessage(t('browse_skipped', { count: outcome.skipped }));
	await loadSubscribed();
}

/** Installed copies go through the usual removal dialog so their files are handled; other items are unsubscribed directly. */
export async function unsubscribeItems(items) {
	const ids = items.map(item => String(item.id));
	const titles = new Map(items.map(item => [String(item.id), item.title]));
	let removable = [];
	try {
		const installed = await invoke('installed_addons_by_id', { ids });
		const removals = installed.length ? await addonRemovals(installed.map(addon => addon.installed.path)) : [];
		removable = installed.filter((addon, index) => removals[index] === 'unsubscribe');
	} catch (error) {
		playSound('error');
		taskError(t('browse_unsubscribe_failed', { error: String(error) }));
		return;
	}
	const handled = new Set(removable.map(addon => String(addon.installed.id)));
	const direct = ids.filter(id => !handled.has(id));
	if (direct.length) {
		let outcome;
		try {
			outcome = await runJob('unsubscribe', direct.length, 'unsubscribe_workshop_items', { ids: direct });
		} catch (error) {
			playSound('error');
			taskError(t('browse_unsubscribe_failed', { error: String(error) }));
			return;
		}
		if (!outcome) return;
		reportOutcome(outcome, 'browse_unsubscribed', outcome.unsubscribed);
		await loadSubscribed();
	}
	if (removable.length) {
		const choices = removable.map(addon => ({ path: addon.installed.path, name: titles.get(String(addon.installed.id)) ?? addonName(null, addon.installed) }));
		confirmRemoval(choices, true, () => {
			loadSubscribed();
			noteInstalled([...handled]);
		});
	}
}

export async function downloadItems(ids) {
	try {
		await invoke('workshop_download', { ids: ids.map(Number) });
	} catch (error) {
		playSound('error');
		taskError(t('browse_download_failed', { error: String(error) }));
		return;
	}
	playSound('success');
	taskMessage(t('sent_to_downloader'));
}

export async function setVote(id, up) {
	try {
		await invoke('set_workshop_vote', { id: String(id), up });
	} catch (error) {
		playSound('error');
		taskError(t('browse_vote_failed', { error: String(error) }));
		return false;
	}
	taskMessage(t(up ? 'browse_voted_up' : 'browse_voted_down'));
	return true;
}

export async function setFavorite(id, favorite) {
	try {
		await invoke('set_workshop_favorite', { id: String(id), favorite });
	} catch (error) {
		playSound('error');
		taskError(t('browse_favorite_failed', { error: String(error) }));
		return false;
	}
	favoriteIds.update(known => {
		const next = new Set(known ?? []);
		favorite ? next.add(String(id)) : next.delete(String(id));
		return next;
	});
	taskMessage(t(favorite ? 'browse_favorited' : 'browse_unfavorited'));
	playSound('success');
	return true;
}

export const browseRequest = writable(null);

export function showInBrowse(id) {
	browseRequest.set(String(id));
}

/**
 * Shows a Workshop item link in Browse when the setting allows it, and returns whether it did.
 * Links in a dialog stay external unless the dialog cancels `workshoplink` because it is closing.
 */
export function openInBrowse(href, link) {
	if (!AppSettings.open_workshop_pages_in_app) return false;
	const widget = bbcodeWidget(href);
	if (widget?.type !== 'workshop') return false;
	if (link.closest('modal') && link.dispatchEvent(new CustomEvent('workshoplink', { bubbles: true, cancelable: true }))) return false;
	showInBrowse(widget.id);
	return true;
}

/** A pasted link or bare item ID opens that item instead of searching. */
export function itemReference(text) {
	const value = text.trim();
	if (/^\d{6,20}$/.test(value)) return value;
	if (/steamcommunity\.com\/(sharedfiles|workshop)\/filedetails\/?\?(.*&)?id=\d+/i.test(value)) return value;
	return null;
}
