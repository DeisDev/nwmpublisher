import { writable } from 'svelte/store';

const openDialogs = writable([]);

export function openDialog(dialog) {
	openDialogs.update(dialogs => [...dialogs.filter(open => open !== dialog), dialog]);
}

export function closeDialog(dialog) {
	openDialogs.update(dialogs => dialogs.filter(open => open !== dialog));
}

// Everything outside an open modal dialog is inert, so app-wide overlays move into the topmost one to stay usable.
export function followDialogs(node) {
	const home = node.parentNode;
	const unsubscribe = openDialogs.subscribe(dialogs => {
		const target = dialogs.findLast(dialog => !node.contains(dialog)) ?? home;
		if (node.parentNode !== target) target.append(node);
	});
	return { destroy: unsubscribe };
}
