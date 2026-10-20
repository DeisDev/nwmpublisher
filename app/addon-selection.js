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
