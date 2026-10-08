function textAnchors(element, start, origin) {
	const points = [];
	const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
	const range = document.createRange();
	let node;
	while ((node = walker.nextNode())) {
		range.selectNodeContents(node);
		const rects = Array.from(range.getClientRects());
		let offset = 0;
		let previousTop = null;
		for (const rect of rects) {
			if (!rect.height || rect.top === previousTop || !node.length) continue;
			let low = offset;
			let high = node.length - 1;
			while (low < high) {
				const middle = Math.floor((low + high) / 2);
				range.setStart(node, middle);
				range.setEnd(node, middle + 1);
				if (range.getBoundingClientRect().top < rect.top - .5) low = middle + 1;
				else high = middle;
			}
			offset = low;
			previousTop = rect.top;
			points.push({ offset: start + offset, top: rect.top - origin });
		}
		start += node.length;
	}
	return points;
}

function scrollAnchors(input, highlight, preview) {
	const origin = preview.getBoundingClientRect().top - preview.scrollTop;
	const points = [];
	for (const element of preview.querySelectorAll('[data-source-start]')) {
		const closed = element.closest('details:not([open])');
		if (closed && closed !== element) continue;
		const start = Number(element.dataset.sourceStart);
		if (element.hasAttribute('data-source-text')) points.push(...textAnchors(element, start, origin));
		else {
			const rect = element.getClientRects()[0];
			if (rect) points.push({ offset: start, top: rect.top - origin });
		}
	}
	points.sort((a, b) => a.offset - b.offset || a.top - b.top);

	const walker = document.createTreeWalker(highlight, NodeFilter.SHOW_TEXT);
	const range = document.createRange();
	const sourceOrigin = highlight.getBoundingClientRect().top - highlight.scrollTop;
	const anchors = [[0, 0]];
	let text = walker.nextNode();
	let offset = 0;
	for (const point of points) {
		while (text && offset + text.length <= point.offset) {
			offset += text.length;
			text = walker.nextNode();
		}
		if (!text) break;
		const index = point.offset - offset;
		range.setStart(text, index);
		range.setEnd(text, Math.min(index + 1, text.length));
		const top = range.getBoundingClientRect().top - sourceOrigin;
		const previous = anchors[anchors.length - 1];
		if (top > previous[0] + .5 && point.top >= previous[1]) anchors.push([top, point.top]);
	}
	anchors.push([input.scrollHeight, preview.scrollHeight]);
	return anchors;
}

function mappedScroll(anchors, position, axis) {
	let low = 0;
	let high = anchors.length - 1;
	while (low + 1 < high) {
		const middle = Math.floor((low + high) / 2);
		if (anchors[middle][axis] <= position) low = middle;
		else high = middle;
	}
	const before = anchors[low];
	const after = anchors[high];
	const distance = after[axis] - before[axis];
	const progress = distance > 0 ? Math.max(0, Math.min(1, (position - before[axis]) / distance)) : 0;
	return before[1 - axis] + progress * (after[1 - axis] - before[1 - axis]);
}

export function syncBBCodeScroll(node, options) {
	const input = node.querySelector('textarea');
	const highlight = node.querySelector('.highlight');
	const preview = node.querySelector('.preview');
	const pending = new Map();
	let anchors = [];
	let frame = 0;
	let source = input;

	function sync() {
		if (!options.enabled || !anchors.length || !input.clientHeight || !preview.clientHeight) return;
		const target = source === input ? preview : input;
		const maximum = source.scrollHeight - source.clientHeight;
		if (maximum <= 0) return;
		const targetMaximum = Math.max(0, target.scrollHeight - target.clientHeight);
		const top = source.scrollTop <= 0 ? 0 : source.scrollTop >= maximum - 1 ? targetMaximum
			: mappedScroll(anchors, source.scrollTop, source === input ? 0 : 1);
		const previous = target.scrollTop;
		target.scrollTop = Math.max(0, Math.min(targetMaximum, top));
		if (target.scrollTop !== previous) pending.set(target, target.scrollTop);
	}

	function refresh() {
		if (frame || !options.enabled) return;
		frame = requestAnimationFrame(() => {
			frame = 0;
			if (!options.enabled || !input.clientHeight || !preview.clientHeight) return;
			anchors = scrollAnchors(input, highlight, preview);
			sync();
		});
	}

	function onScroll(event) {
		const scrolled = event.currentTarget;
		const expected = pending.get(scrolled);
		pending.delete(scrolled);
		if (expected !== undefined && Math.abs(expected - scrolled.scrollTop) < 1) return;
		source = scrolled;
		if (!frame) sync();
	}

	const observer = new ResizeObserver(refresh);
	observer.observe(input);
	observer.observe(preview);
	observer.observe(preview.firstElementChild);
	input.addEventListener('scroll', onScroll);
	preview.addEventListener('scroll', onScroll);
	preview.addEventListener('load', refresh, true);
	preview.addEventListener('toggle', refresh, true);
	refresh();

	return {
		update(next) {
			if (next.value !== options.value || next.enabled !== options.enabled) source = input;
			options = next;
			refresh();
		},
		destroy() {
			cancelAnimationFrame(frame);
			observer.disconnect();
			input.removeEventListener('scroll', onScroll);
			preview.removeEventListener('scroll', onScroll);
			preview.removeEventListener('load', refresh, true);
			preview.removeEventListener('toggle', refresh, true);
		},
	};
}
