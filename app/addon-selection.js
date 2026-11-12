import { writable } from 'svelte/store';

export function createSelection() {
	const { subscribe, set, update } = writable({ active: false, items: new Map() });

	function edit(change) {
		update(state => {
			const items = new Map(state.items);
			change(items);
			return { active: true, items };
		});
	}

	return {
		subscribe,
		start: () => update(state => (state.active ? state : { ...state, active: true })),
		stop: () => set({ active: false, items: new Map() }),
		clear: () => update(state => ({ ...state, items: new Map() })),
		toggle: (key, value) => edit(items => (items.has(key) ? items.delete(key) : items.set(key, value))),
		add: entries => edit(items => entries.forEach(([key, value]) => items.set(key, value))),
		setAll: entries => set({ active: true, items: new Map(entries) }),
		remove: keys => update(state => {
			if (!keys.some(key => state.items.has(key))) return state;
			const items = new Map(state.items);
			keys.forEach(key => items.delete(key));
			return { ...state, items };
		}),
	};
}

const NAVIGATION_KEYS = new Set(['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End']);

/** Whether a navigation key pressed elsewhere on the page belongs to `region`: it is on screen and nothing else uses the key. */
export function takesNavigationKey(event, region) {
	if (event.defaultPrevented || event.altKey || !NAVIGATION_KEYS.has(event.key)) return false;
	if (!region || region.offsetParent === null || region.closest('[inert]') || region.contains(event.target)) return false;
	return !document.querySelector('modal.active') && !event.target.closest?.('input, select, textarea, [contenteditable], [role="menu"]');
}

/** Where keys entering a list start: `current` while it is on screen, otherwise the first option that is. */
export function entryIndex(options, current, scroller) {
	const bounds = scroller.getBoundingClientRect();
	const onScreen = option => {
		const rect = option.getBoundingClientRect();
		const middle = (rect.top + rect.bottom) / 2;
		return middle >= bounds.top && middle <= bounds.bottom;
	};
	if (options[current] && onScreen(options[current])) return current;
	return Math.max(options.findIndex(onScreen), 0);
}

/** The index a navigation key moves to in a grid that fills rows from the start, or null for other keys. */
export function moveIndex(key, index, count, columns, pageRows, rtl) {
	const last = count - 1;
	const lastRow = Math.floor(last / columns);
	const down = rows => {
		const target = index + rows * columns;
		return target <= last ? target : Math.floor(index / columns) < lastRow ? last : index;
	};
	switch (key) {
		case 'ArrowLeft':
			return Math.min(Math.max(index + (rtl ? 1 : -1), 0), last);
		case 'ArrowRight':
			return Math.min(Math.max(index + (rtl ? -1 : 1), 0), last);
		case 'ArrowUp':
			return index >= columns ? index - columns : index;
		case 'ArrowDown':
			return down(1);
		case 'PageUp':
			return Math.max(index - pageRows * columns, index % columns);
		case 'PageDown':
			return down(pageRows);
		case 'Home':
			return 0;
		case 'End':
			return last;
		default:
			return null;
	}
}
