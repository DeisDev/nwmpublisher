<script context="module">
	// Kept across visits so the page reopens instantly; the installed addons revision invalidates it.
	const cache = { key: null, analysis: null };
	const workshopItems = new Map();
	const previews = new Map();
	const tagColors = {};
	let includeFolders = false;
</script>

<script>
	import { onDestroy, onMount } from 'svelte';
	import { writable } from 'svelte/store';
	import { _, locale } from 'svelte-i18n';
	import { invoke } from '@tauri-apps/api/core';
	import Search from '@lucide/svelte/icons/search';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Folder from '@lucide/svelte/icons/folder';
	import { Transaction } from '../transactions.js';
	import tippy from 'tippy.js';
	import { formatPercent, formatSize } from '../format.js';
	import Loading from '../components/Loading.svelte';
	import Dead from '../components/Dead.svelte';
	import { Steam, installedAddonsRevision } from '../steam';
	import { removedPaths } from '../addon-removal.js';
	import PreviewGMA from '../components/PreviewGMA.svelte';
	import { registerContext } from '../components/ContextMenu.svelte';

	const MIN_TILE_AREA = 64;

	function tagClass(tag) {
		return tag.toLowerCase();
	}

	function getTagColor(tag) {
		const className = tagClass(tag);
		if (!/^[a-z0-9_-]+$/.test(className)) return null;

		if (!(className in tagColors)) {
			const elem = document.createElement('span');
			elem.classList.add('tag', className);
			document.body.appendChild(elem);

			const color = window.getComputedStyle(elem, '::before')?.getPropertyValue('background-color');
			tagColors[className] = !!color ? color : null;

			elem.remove();
		}

		return tagColors[className];
	}

	function placeholderColor() {
		return window.getComputedStyle(main).getPropertyValue('--bg-content').trim();
	}

	let main;
	let sizeRef;
	let searchInput;

	let addonsCanvas; let addonsCtx;
	let tagsCanvas; let tagsCtx;
	let bgCanvas; let bgCtx;

	let loading = new Promise(() => {});

	let transaction;
	let progressMsg = writable(null);

	let analysis = null;
	let zoomTag = null;
	let query = '';
	let folders = includeFolders;
	let layoutBounds = [0, 0];
	let tiles = [];
	let tilesById = new Map();
	let tagLabels = {};
	let focusedTile = null;

	const DeadSVG = new Image();
	DeadSVG.onload = () => { if (tiles.length) drawAddons(); };
	DeadSVG.src = '/img/dead-canvas.svg';

	function cacheKey() {
		return `${$installedAddonsRevision}:${folders}`;
	}

	function setAnalysis(data, key) {
		analysis = data;
		cache.analysis = data;
		cache.key = key;
	}

	function signature(data) {
		if (!data) return null;
		return JSON.stringify([data.totalSize, data.tags.map(group => [group.tag, group.size, group.items.map(item => itemPath(item))])]);
	}

	function analyze({ silent = false, rescan = false } = {}) {
		transaction?.cancel();
		const key = cacheKey();

		invoke('addon_size_analyzer', { includeFolders: folders, rescan }).then(transactionId => {
			transaction = new Transaction(transactionId);

			const run = new Promise((resolve, reject) => {
				transaction.listen(event => {
					if (event.finished) {
						transaction.progress = 100;
						transaction.setStatus('FS_ANALYZER_RENDERING');
						if (!silent || signature(event.data) !== signature(analysis)) {
							setAnalysis(event.data, key);
						} else {
							cache.key = key;
						}
						resolve();
					} else if (event.error != null) {
						reject(event.error);
					}
					$progressMsg = transaction.status;
					transaction = transaction;
				});
			});

			if (silent) {
				run.catch(() => { loading = run; });
			} else {
				loading = run;
			}
		});
	}

	function refresh() {
		zoomTag = null;
		analyze({ rescan: true });
	}

	function toggleFolders(checked) {
		folders = checked;
		includeFolders = checked;
		zoomTag = null;
		if (cache.analysis && cache.key === cacheKey()) {
			analysis = cache.analysis;
			loading = Promise.resolve();
		} else {
			analyze();
		}
	}

	function itemPath(item) {
		return item.archive?.installed.path ?? item.folder?.path ?? null;
	}

	function removeItems(paths) {
		if (!analysis || !paths?.length) return;
		const removed = new Set(paths);

		let changed = false;
		const tags = [];
		for (const group of analysis.tags) {
			const items = group.items.filter(item => !removed.has(itemPath(item)));
			if (items.length === group.items.length) {
				tags.push(group);
				continue;
			}
			changed = true;
			if (!items.length) continue;
			tags.push({ tag: group.tag, size: items.reduce((sum, item) => sum + Number(item.size), 0), items });
		}
		if (!changed) return;

		tags.sort((a, b) => b.size - a.size);
		if (zoomTag && !tags.some(group => group.tag === zoomTag)) zoomTag = null;
		setAnalysis({ totalSize: tags.reduce((sum, group) => sum + group.size, 0), tags }, cache.key);
	}
	$: removeItems($removedPaths);

	let resizeTimeout;
	onMount(() => {
		const observer = new ResizeObserver(() => {
			if (!analysis) return;
			if (layoutBounds[0] === sizeRef.clientWidth && layoutBounds[1] === sizeRef.clientHeight) return;
			clearTimeout(resizeTimeout);
			resizeTimeout = setTimeout(render, 100);
		});
		observer.observe(sizeRef);

		const themeObserver = new MutationObserver(() => {
			if (tiles.length) drawAddons();
		});
		themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });

		return () => {
			observer.disconnect();
			themeObserver.disconnect();
			clearTimeout(resizeTimeout);
		};
	});

	function worstAspectRatio(area, largest, smallest, side) {
		const areaSquared = area * area;
		const sideSquared = side * side;
		return Math.max((sideSquared * largest) / areaSquared, areaSquared / (sideSquared * smallest));
	}

	// Squarified treemap (Bruls, Huizing & van Wijk): sizes must arrive sorted largest first.
	function squarify(sizes, x, y, w, h) {
		const rects = new Array(sizes.length).fill(null);

		let total = 0;
		for (const size of sizes) if (size > 0) total += size;
		if (!(total > 0) || !(w > 0) || !(h > 0)) return rects;

		const scale = (w * h) / total;
		const items = [];
		for (let i = 0; i < sizes.length; i++) {
			if (sizes[i] > 0) items.push({ index: i, area: sizes[i] * scale });
		}

		let row = [];
		let rowArea = 0;
		let rowLargest = 0;
		let rowSmallest = Infinity;

		function layoutRow() {
			if (w >= h) {
				const columnWidth = rowArea / h;
				let cursor = y;
				for (const item of row) {
					const height = item.area / columnWidth;
					rects[item.index] = { x, y: cursor, w: columnWidth, h: height };
					cursor += height;
				}
				x += columnWidth;
				w = Math.max(w - columnWidth, 0);
			} else {
				const rowHeight = rowArea / w;
				let cursor = x;
				for (const item of row) {
					const width = item.area / rowHeight;
					rects[item.index] = { x: cursor, y, w: width, h: rowHeight };
					cursor += width;
				}
				y += rowHeight;
				h = Math.max(h - rowHeight, 0);
			}

			row = [];
			rowArea = 0;
			rowLargest = 0;
			rowSmallest = Infinity;
		}

		for (const item of items) {
			if (!(w > 0) || !(h > 0)) break;

			const side = Math.min(w, h);
			if (row.length > 0) {
				const current = worstAspectRatio(rowArea, rowLargest, rowSmallest, side);
				const extended = worstAspectRatio(rowArea + item.area, Math.max(rowLargest, item.area), Math.min(rowSmallest, item.area), side);
				if (extended > current) layoutRow();
			}

			row.push(item);
			rowArea += item.area;
			rowLargest = Math.max(rowLargest, item.area);
			rowSmallest = Math.min(rowSmallest, item.area);
		}
		if (row.length > 0) layoutRow();

		return rects;
	}

	// Items too small to see are shown as one tile; they arrive sorted, so they are the tail of the list.
	function groupSmallItems(items, area, totalSize) {
		const entries = [];
		const small = [];
		const threshold = area > 0 && totalSize > 0 ? (MIN_TILE_AREA * totalSize) / area : 0;

		for (const item of items) {
			const size = Number(item.size);
			if (size < threshold) small.push(item);
			else entries.push({ size, item });
		}

		if (small.length < 2) {
			for (const item of small) entries.push({ size: Number(item.size), item });
			return entries;
		}

		const group = { size: small.reduce((sum, item) => sum + Number(item.size), 0), group: small };
		let index = entries.findIndex(entry => entry.size < group.size);
		if (index === -1) index = entries.length;
		entries.splice(index, 0, group);
		return entries;
	}

	function tagLabel(tag) {
		return $_('addon_types.' + tag);
	}

	function addonCount(data) {
		return data.tags.reduce((count, group) => count + group.items.length, 0);
	}

	function shareLabel(size, total, selectedLocale) {
		if (!(total > 0)) return '';
		const ratio = size / total;
		return ratio > 0 && ratio < 0.01 ? '<' + formatPercent(0.01, selectedLocale) : formatPercent(ratio, selectedLocale);
	}

	function layout(width, height) {
		tiles = [];
		tilesById = new Map();
		tagLabels = {};

		const groups = zoomTag ? analysis.tags.filter(group => group.tag === zoomTag) : analysis.tags;
		const groupRects = zoomTag ? [{ x: 0, y: 0, w: width, h: height }] : squarify(groups.map(group => Number(group.size)), 0, 0, width, height);

		groups.forEach((group, i) => {
			const rect = groupRects[i];
			if (!rect) return;

			const padding = zoomTag ? 0 : Math.ceil(Math.min(rect.w, rect.h) * 0.05);
			const label = tagLabel(group.tag);
			tagLabels[group.tag] = {
				tag: group.tag,
				label,
				labelled: !zoomTag,
				x: rect.x,
				y: rect.y,
				w: rect.w,
				h: rect.h,
				right: rect.x + rect.w,
				bottom: rect.y + rect.h,
				centerX: rect.x + rect.w / 2,
				centerY: rect.y + rect.h / 2,
				padding: padding / 2,
				hanging: /[gjpqy]/.test(label),
			};

			const innerWidth = Math.max(rect.w - padding, 0);
			const innerHeight = Math.max(rect.h - padding, 0);
			const entries = groupSmallItems(group.items, innerWidth * innerHeight, Number(group.size));
			const entryRects = squarify(entries.map(entry => entry.size), rect.x + padding / 2, rect.y + padding / 2, innerWidth, innerHeight);

			entries.forEach((entry, j) => {
				const entryRect = entryRects[j];
				if (!entryRect) return;

				const tile = {
					item: entry.item ?? null,
					group: entry.group ?? null,
					size: entry.size,
					tag: group.tag,
					x: entryRect.x,
					y: entryRect.y,
					w: entryRect.w,
					h: entryRect.h,
					left: entryRect.x,
					right: entryRect.x + entryRect.w,
					top: entryRect.y,
					bottom: entryRect.y + entryRect.h,
					centerX: entryRect.x + entryRect.w / 2,
					centerY: entryRect.y + entryRect.h / 2,
				};
				tiles.push(tile);
				const id = tileId(tile);
				if (id != null) tilesById.set(id, tile);
			});
		});

		if (focusedTile) {
			const path = tilePath(focusedTile);
			focusedTile = tiles.find(tile => tile.item && tilePath(tile) === path) ?? null;
		}
	}

	function tileId(tile) {
		return tile.item?.archive?.installed.id ?? null;
	}

	function tilePath(tile) {
		return tile.item ? itemPath(tile.item) : null;
	}

	function tileTitle(tile) {
		if (tile.group) return $_('analyzer_small_group', { values: { count: tile.group.length } });
		const archive = tile.item.archive?.installed;
		if (archive) {
			const id = archive.id;
			const item = id != null ? workshopItems.get(id) : null;
			const workshopTitle = item && !item.dead ? item.title : null;
			return workshopTitle ?? archive.title ?? archive.extractedName ?? String(id ?? '');
		}
		return tile.item.folder.title;
	}

	function setupCanvas(canvas, width, height, scale) {
		canvas.width = Math.floor(width * scale);
		canvas.height = Math.floor(height * scale);
		const ctx = canvas.getContext('2d');
		ctx.setTransform(scale, 0, 0, scale, 0, 0);
		return ctx;
	}

	function canvasesReady() {
		return !!(bgCanvas && addonsCanvas && tagsCanvas && bgCtx && addonsCtx && tagsCtx);
	}

	// The canvases unmount whenever the loading screen shows, so their contexts must not be drawn to.
	$: if (!bgCanvas || !addonsCanvas || !tagsCanvas) {
		bgCtx = null;
		addonsCtx = null;
		tagsCtx = null;
		tagsDrawn = null;
	}

	function render() {
		if (!analysis || !bgCanvas || !addonsCanvas || !tagsCanvas || !sizeRef) return;

		const width = sizeRef.clientWidth;
		const height = sizeRef.clientHeight;
		if (!(width > 0) || !(height > 0)) return;
		layoutBounds = [width, height];

		hideTooltip(true);
		layout(width, height);

		const scale = window.devicePixelRatio || 1;
		bgCtx = setupCanvas(bgCanvas, width, height, scale);
		addonsCtx = setupCanvas(addonsCanvas, width, height, scale);
		tagsCtx = setupCanvas(tagsCanvas, width, height, scale);
		tagsCtx.textAlign = 'center';
		tagsCtx.textBaseline = 'middle';
		tagsCtx.lineJoin = 'round';

		drawBackground();
		drawAddons();
		drawTags(null, true);
		if (focusedTile) showTooltip(focusedTile);
	}

	function drawBackground() {
		for (const tag in tagLabels) {
			const label = tagLabels[tag];
			bgCtx.fillStyle = getTagColor(tag) ?? placeholderColor();
			bgCtx.fillRect(label.x, label.y, label.w, label.h);
		}
	}

	function drawAddons() {
		if (!canvasesReady()) return;
		addonsCtx.clearRect(0, 0, addonsCanvas.width, addonsCanvas.height);
		const placeholder = placeholderColor();
		const words = queryWords(query);
		for (const tile of tiles) drawTile(tile, placeholder, words);
	}

	function drawTile(tile, placeholder = placeholderColor(), words = queryWords(query)) {
		const id = tileId(tile);
		const preview = id != null ? previews.get(id) : undefined;

		if (preview && preview !== 'loading') {
			refreshPreviewIfSmall(id, preview, tile);
			addonsCtx.drawImage(preview.canvas, tile.x, tile.y, tile.w, tile.h);
		} else {
			if (id != null && !previews.has(id)) requestPreview(id);

			addonsCtx.fillStyle = placeholder;
			addonsCtx.fillRect(tile.x, tile.y, tile.w, tile.h);

			if (tile.group) {
				drawCenteredText(addonsCtx, '+' + tile.group.length, tile);
			} else if (DeadSVG.complete && DeadSVG.naturalWidth > 0) {
				const size = Math.min(tile.w, tile.h) * .4;
				addonsCtx.drawImage(DeadSVG, tile.x + (tile.w - size) / 2, tile.y + (tile.h - size) / 2, size, size);
			}
		}

		if (words.length && !tileMatches(tile, words)) {
			addonsCtx.fillStyle = 'rgba(0, 0, 0, .7)';
			addonsCtx.fillRect(tile.x, tile.y, tile.w, tile.h);
		}
	}

	function drawCenteredText(ctx, text, tile) {
		const size = fitFontSize(ctx, text, tile.w * .75, tile.h * .75, false);
		if (size < 6) return;
		ctx.save();
		ctx.textAlign = 'center';
		ctx.textBaseline = 'middle';
		ctx.lineJoin = 'round';
		ctx.font = size + 'px sans-serif';
		ctx.fillStyle = '#fff';
		ctx.strokeStyle = '#000';
		ctx.lineWidth = Math.max(Math.ceil(size * 0.15), 2);
		ctx.strokeText(text, tile.centerX, tile.centerY);
		ctx.fillText(text, tile.centerX, tile.centerY);
		ctx.restore();
	}

	function requestPreview(id) {
		previews.set(id, 'loading');
		Steam.getWorkshopAddon(id).then(item => {
			workshopItems.set(id, item);
			if (item && item.previewUrl) {
				decodePreview(id, item.previewUrl);
			} else {
				previews.set(id, null);
			}
		}).catch(() => previews.set(id, null));
	}

	function decodePreview(id, url) {
		const image = new Image();
		image.onload = () => {
			const tile = tilesById.get(id);
			if (!tile) {
				previews.delete(id);
				return;
			}
			previews.set(id, downscale(image, tile, url));
			if (canvasesReady()) drawTile(tile);
		};
		image.onerror = () => {
			const current = previews.get(id);
			if (current && current !== 'loading') current.refreshing = false;
			else previews.set(id, null);
		};
		image.src = url;
	}

	function neededPreviewSize(tile) {
		const scale = window.devicePixelRatio || 1;
		return [Math.max(1, Math.ceil(tile.w * scale)), Math.max(1, Math.ceil(tile.h * scale))];
	}

	function downscale(image, tile, url) {
		const [neededWidth, neededHeight] = neededPreviewSize(tile);
		const width = Math.min(image.naturalWidth, neededWidth);
		const height = Math.min(image.naturalHeight, neededHeight);

		const canvas = document.createElement('canvas');
		canvas.width = width;
		canvas.height = height;
		canvas.getContext('2d').drawImage(image, 0, 0, width, height);

		return { canvas, width, height, url, refreshing: false };
	}

	function refreshPreviewIfSmall(id, preview, tile) {
		if (preview.refreshing) return;
		const [neededWidth, neededHeight] = neededPreviewSize(tile);
		if (preview.width * 2 >= neededWidth && preview.height * 2 >= neededHeight) return;
		preview.refreshing = true;
		decodePreview(id, preview.url);
	}

	const canvasRotation = -90 * Math.PI / 180;

	function fitFontSize(ctx, text, boundsWidth, boundsHeight, hanging) {
		const fits = size => {
			ctx.font = size + 'px sans-serif';
			const metrics = ctx.measureText(text);
			const height = hanging ? ((metrics.fontBoundingBoxAscent ?? 0) + (metrics.fontBoundingBoxDescent ?? 0)) : ((metrics.actualBoundingBoxAscent ?? 0) + (metrics.actualBoundingBoxDescent ?? 0));
			return metrics.width < boundsWidth && height < boundsHeight;
		};

		let low = 1;
		let high = Math.max(1, Math.floor(Math.max(boundsWidth, boundsHeight)));
		while (low < high) {
			const mid = Math.ceil((low + high) / 2);
			if (fits(mid)) low = mid;
			else high = mid - 1;
		}
		return low;
	}

	function drawTag(tag) {
		const { w, h, centerX: x, centerY: y, padding, hanging, label } = tagLabels[tag];

		const verticalText = w / h <= 0.33;
		const textBoundsWidth = Math.floor((w - padding) * .75);
		const textBoundsHeight = Math.floor((h - padding) * .75);
		const textSize = verticalText
			? fitFontSize(tagsCtx, label, textBoundsHeight, textBoundsWidth, hanging)
			: fitFontSize(tagsCtx, label, textBoundsWidth, textBoundsHeight, hanging);

		if (verticalText) {
			tagsCtx.save();
			tagsCtx.translate(x, y);
				tagsCtx.rotate(canvasRotation);
			tagsCtx.translate(-x, -y);
		}

		tagsCtx.font = textSize + 'px sans-serif';
		tagsCtx.fillStyle = '#fff';
		tagsCtx.strokeStyle = '#000';
		tagsCtx.lineWidth = Math.min(Math.max(Math.ceil(textSize * 0.2), 4), (14 / 1920) * window.innerWidth);
		tagsCtx.strokeText(label, x, y);
		tagsCtx.fillText(label, x, y);

		if (verticalText) {
			tagsCtx.restore();
		}
	}

	// Labels are only redrawn when the hidden label or the focus ring changes.
	let tagsDrawn = null;
	function drawTags(filter = null, force = false) {
		if (!canvasesReady()) return;

		const ring = focusedTile && document.activeElement === tagsCanvas ? focusedTile : null;
		const key = `${filter ?? ''}|${ring ? [ring.x, ring.y, ring.w, ring.h].join(',') : ''}`;
		if (!force && key === tagsDrawn) return;
		tagsDrawn = key;

		tagsCtx.clearRect(0, 0, tagsCanvas.width, tagsCanvas.height);

		for (const tag in tagLabels) {
			if (tagLabels[tag].labelled && tag !== filter) drawTag(tag);
		}

		if (ring) {
			tagsCtx.strokeStyle = '#127cff';
			tagsCtx.lineWidth = 2;
			tagsCtx.strokeRect(ring.x + 1, ring.y + 1, Math.max(ring.w - 2, 0), Math.max(ring.h - 2, 0));
		}
	}

	function queryWords(text) {
		return text.trim().toLowerCase().split(/\s+/).filter(Boolean);
	}

	function itemHaystack(item) {
		const parts = [];
		const archive = item.archive?.installed;
		if (archive) {
			if (archive.id != null) {
				parts.push(String(archive.id));
				const workshop = workshopItems.get(archive.id);
				if (workshop && !workshop.dead && workshop.title) parts.push(workshop.title);
			}
			if (archive.title) parts.push(archive.title);
			if (archive.extractedName) parts.push(archive.extractedName);
			parts.push(archive.path.split(/[\\/]/).pop() ?? '');
		} else if (item.folder) {
			parts.push(item.folder.title, item.folder.path.split(/[\\/]/).pop() ?? '');
		}
		return parts.join('\n').toLowerCase();
	}

	function itemMatches(item, words) {
		const haystack = itemHaystack(item);
		return words.every(word => haystack.includes(word));
	}

	function tileMatches(tile, words) {
		if (!words.length) return true;
		if (tile.group) return tile.group.some(item => itemMatches(item, words));
		return itemMatches(tile.item, words);
	}

	let matchCount = 0;
	function updateSearch(text) {
		const words = queryWords(text);
		matchCount = words.length && analysis ? analysis.tags.reduce((count, group) => count + group.items.filter(item => itemMatches(item, words)).length, 0) : 0;
		if (tiles.length) drawAddons();
	}
	$: updateSearch(query, analysis);

	function findTileAt(offsetX, offsetY) {
		for (const tile of tiles) {
			if (offsetX >= tile.left && offsetX < tile.right && offsetY >= tile.top && offsetY < tile.bottom) return tile;
		}
		return null;
	}

	function findTagAt(offsetX, offsetY) {
		for (const tag in tagLabels) {
			const label = tagLabels[tag];
			if (offsetX >= label.x && offsetX < label.right && offsetY >= label.y && offsetY < label.bottom) return tag;
		}
		return null;
	}

	function canvasHover(e) {
		const hovered = findTileAt(e.offsetX, e.offsetY);
		if (hovered) showTooltip(hovered);
		else if (focusedTile) showTooltip(focusedTile);
		else hideTooltip();
	}

	function zoomInto(tag) {
		zoomTag = zoomTag === tag ? null : tag;
		focusedTile = null;
		render();
	}

	function zoomOut() {
		if (!zoomTag) return;
		zoomTag = null;
		focusedTile = null;
		render();
	}

	$: if (analysis && bgCanvas && addonsCanvas && tagsCanvas) render();

	onMount(() => {
		if (cache.analysis && cache.key === cacheKey()) {
			analysis = cache.analysis;
			loading = Promise.resolve();
		} else {
			analyze();
		}
	});
	let analyzedRevision = $installedAddonsRevision;
	$: if ($installedAddonsRevision !== analyzedRevision) {
		analyzedRevision = $installedAddonsRevision;
		analyze({ silent: analysis != null });
	}
	onDestroy(() => transaction?.cancel());

	let popper;
	let popperContent;
	let popperName;
	let popperType;
	let popperSize;
	let popperShare;
	let popperIdRow;
	let popperId;
	let popperPathRow;
	let popperPath;

	let tooltip;
	let activeTile = null;
	let activeRect = null;
	let hideTimeout;

	onMount(() => {
		tooltip = tippy(popper, {
			allowHTML: true,
			arrow: true,
			interactive: false,
			trigger: 'manual',
			hideOnClick: false,
			duration: [120, 80],
			moveTransition: 'transform .12s ease-out',
			getReferenceClientRect: () => activeRect ?? popper.getBoundingClientRect(),
		});
		return () => {
			clearTimeout(hideTimeout);
			tooltip.destroy();
			tooltip = null;
		};
	});

	function tileClientRect(tile) {
		const canvas = tagsCanvas.getBoundingClientRect();
		return new DOMRect(canvas.left + tile.left, canvas.top + tile.top, tile.w, tile.h);
	}

	function fillTooltip(tile, selectedLocale = $locale) {
		const archive = tile.item?.archive?.installed;
		popperName.textContent = tileTitle(tile);
		popperType.setAttribute('class', 'tag ' + tagClass(tile.tag));
		popperType.textContent = tagLabel(tile.tag);
		popperSize.textContent = formatSize(Number(tile.size), selectedLocale);
		popperShare.textContent = analysis?.totalSize > 0 ? formatPercent(Number(tile.size) / Number(analysis.totalSize), selectedLocale) : '';
		popperIdRow.hidden = archive?.id == null;
		popperId.textContent = archive?.id != null ? String(archive.id) : '';
		const path = tile.item ? itemPath(tile.item) : null;
		popperPathRow.hidden = path == null;
		popperPath.textContent = path ?? '';
		tooltip.setContent(popperContent.innerHTML);
	}
	function relocalizeTooltip(selectedLocale) {
		if (activeTile && tooltip) fillTooltip(activeTile, selectedLocale);
	}
	$: relocalizeTooltip($locale);

	function showTooltip(tile) {
		clearTimeout(hideTimeout);
		if (!tooltip || !tagsCanvas) return;
		if (tile === activeTile && tooltip.state.isVisible) return;

		const wasActive = activeTile != null;
		activeTile = tile;
		activeRect = tileClientRect(tile);

		fillTooltip(tile);
		drawTags(tile.tag);

		// The ring only glides when it moves between tiles, not when it appears.
		if (!wasActive) popper.style.transition = 'none';
		popper.style.boxShadow = `inset 0 0 0 .25rem ${getTagColor(tile.tag) ?? 'var(--bg-content)'}`;
		popper.style.left = tile.left + 'px';
		popper.style.top = tile.top + 'px';
		popper.style.width = tile.w + 'px';
		popper.style.height = tile.h + 'px';
		if (!wasActive) {
			void popper.offsetWidth;
			popper.style.transition = '';
		}
		popper.style.opacity = '1';

		tooltip.setProps({ getReferenceClientRect: () => activeRect });
		if (!tooltip.state.isVisible) tooltip.show();
	}

	function hideTooltip(immediately = false) {
		clearTimeout(hideTimeout);
		if (!activeTile) return;
		const hide = () => {
			activeTile = null;
			activeRect = null;
			if (popper) popper.style.opacity = '0';
			tooltip?.hide();
			drawTags(focusedTile?.tag ?? null);
		};
		if (immediately) hide();
		else hideTimeout = setTimeout(hide, 80);
	}

	let previewingGMA = false;
	const promises = writable([new Promise(() => {}), new Promise(() => {})]);
	function openTile(tile) {
		hideTooltip(true);
		if (!tile) return;
		if (tile.group) {
			if (!zoomTag) zoomInto(tile.tag);
			return;
		}
		const archive = tile.item.archive?.installed;
		if (archive) {
			previewingGMA = true;
			$promises = [archive.id != null ? Steam.getWorkshopAddon(archive.id) : null, Promise.resolve(archive)];
		} else {
			invoke('open_file_location', { path: tile.item.folder.path });
		}
	}

	function canvasClicked(e) {
		const tile = findTileAt(e.offsetX, e.offsetY);
		if (tile) {
			openTile(tile);
			return;
		}
		const tag = findTagAt(e.offsetX, e.offsetY);
		if (tag && !zoomTag) zoomInto(tag);
	}

	function neighbour(from, key) {
		let best = null;
		let bestScore = Infinity;
		for (const tile of tiles) {
			if (tile === from) continue;
			let gap;
			let offset;
			switch (key) {
				case 'ArrowRight': gap = tile.left - from.right; offset = Math.max(0, tile.top - from.centerY, from.centerY - tile.bottom); break;
				case 'ArrowLeft': gap = from.left - tile.right; offset = Math.max(0, tile.top - from.centerY, from.centerY - tile.bottom); break;
				case 'ArrowDown': gap = tile.top - from.bottom; offset = Math.max(0, tile.left - from.centerX, from.centerX - tile.right); break;
				case 'ArrowUp': gap = from.top - tile.bottom; offset = Math.max(0, tile.left - from.centerX, from.centerX - tile.right); break;
				default: return null;
			}
			if (gap < -1) continue;
			const score = Math.max(gap, 0) + offset * 2;
			if (score < bestScore) {
				bestScore = score;
				best = tile;
			}
		}
		return best;
	}

	function focusTile(tile) {
		focusedTile = tile;
		if (tile) showTooltip(tile);
		else hideTooltip(true);
		drawTags(tile ? tile.tag : null);
	}

	function openContextMenu(tile) {
		const rect = tagsCanvas.getBoundingClientRect();
		tagsCanvas.dispatchEvent(new MouseEvent('contextmenu', {
			bubbles: true,
			cancelable: true,
			clientX: rect.left + tile.centerX,
			clientY: rect.top + tile.centerY,
		}));
	}

	function canvasKeydown(e) {
		if (!tiles.length) return;
		switch (e.key) {
			case 'ArrowRight':
			case 'ArrowLeft':
			case 'ArrowDown':
			case 'ArrowUp': {
				e.preventDefault();
				const next = focusedTile ? neighbour(focusedTile, e.key) : tiles[0];
				if (next) focusTile(next);
				break;
			}
			case 'Home':
				e.preventDefault();
				focusTile(tiles[0]);
				break;
			case 'End':
				e.preventDefault();
				focusTile(tiles[tiles.length - 1]);
				break;
			case 'Enter':
			case ' ':
				if (!focusedTile) return;
				e.preventDefault();
				openTile(focusedTile);
				break;
			case 'Escape':
				e.preventDefault();
				if (zoomTag) zoomOut();
				else focusTile(null);
				break;
			case 'ContextMenu':
			case 'F10':
				if (e.key === 'F10' && !e.shiftKey) return;
				if (!focusedTile || !focusedTile.item) return;
				e.preventDefault();
				openContextMenu(focusedTile);
				break;
		}
	}

	function windowKeydown(e) {
		if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'f' && (e.target === document.body || main?.contains(e.target))) {
			e.preventDefault();
			searchInput?.focus();
			searchInput?.select();
		}
	}

	function searchKeydown(e) {
		if (e.key === 'Escape') {
			e.preventDefault();
			query = '';
		}
	}

	let timeouts = [];
	onDestroy(() => timeouts.map(clearTimeout));

	let progressLog;
	progressMsg.subscribe(msg => {
		if (!!!msg || !progressLog) return;

		if (progressLog.firstElementChild) {
			const elem = progressLog.firstElementChild;
			elem.classList.add('stale');
			timeouts.push(setTimeout(elem => {
				if (!elem) return;
				elem.remove();
			}, 2500, elem));
		}

		const elem = document.createElement('div');
		elem.classList.add('log');
		elem.textContent = $_(msg);
		progressLog.prepend(elem);
	});
	onMount(() => $progressMsg = $progressMsg);

	onMount(() => {
		registerContext(main, e => {
			const hovered = findTileAt(e.offsetX, e.offsetY);
			if (!hovered || hovered.group) return;
			const archive = hovered.item.archive?.installed;
			if (archive) {
				return [archive.id != null ? Steam.getWorkshopAddon(archive.id) : null, Promise.resolve(archive), true];
			}
			const path = hovered.item.folder.path;
			return [null, null, true, [{ icon: Folder, label: $_('open_folder'), click: () => invoke('open_file_location', { path }) }]];
		});
	});
</script>

<svelte:window on:keydown={windowKeydown}/>

<PreviewGMA active={previewingGMA} {promises} cancel={() => previewingGMA = false}/>

<div id="popper-content" bind:this={popperContent}>
	<div class="popper-content">
		<div>{$_('name')}</div>
		<div bind:this={popperName}></div>

		<div>{$_('addon_type')}</div>
		<div><div bind:this={popperType}></div></div>

		<div>{$_('size')}</div>
		<div bind:this={popperSize}></div>

		<div>{$_('analyzer_share')}</div>
		<div bind:this={popperShare}></div>

		<div class="row" bind:this={popperIdRow}>
			<div>{$_('analyzer_workshop_id')}</div>
			<div bind:this={popperId}></div>
		</div>

		<div class="row" bind:this={popperPathRow}>
			<div>{$_('analyzer_path')}</div>
			<div class="path" bind:this={popperPath}></div>
		</div>
	</div>
</div>

<section bind:this={main}>
	<div class="toolbar">
		<div class="heading">
			{#if analysis}
				{#if zoomTag}
					<button type="button" class="plain" on:click={zoomOut}>
						<span class="total">{formatSize(Number(analysis.totalSize), $locale)}</span>
						<span class="count">{$_('analyzer_count', { values: { count: addonCount(analysis) } })}</span>
					</button>
					<ChevronRight size="1rem"/>
					<span class="tag {tagClass(zoomTag)}">{tagLabel(zoomTag)}</span>
				{:else}
					<span class="total">{formatSize(Number(analysis.totalSize), $locale)}</span>
					<span class="count">{$_('analyzer_count', { values: { count: addonCount(analysis) } })}</span>
				{/if}
				{#if queryWords(query).length}
					<span class="count">· {$_('analyzer_matches', { values: { count: matchCount } })}</span>
				{/if}
			{:else}
				<span class="total">{$_('size_analyzer')}</span>
			{/if}
		</div>
		<div class="controls">
			<div class="search">
				<Search size="1rem"/>
				<input type="search" bind:value={query} bind:this={searchInput} placeholder={$_('analyzer_search')} aria-label={$_('analyzer_search')} on:keydown={searchKeydown}/>
			</div>
			<label class="folders">
				<input type="checkbox" checked={folders} on:change={e => toggleFolders(e.currentTarget.checked)}/>
				{$_('analyzer_include_folders')}
			</label>
			<button type="button" on:click={refresh} title={$_('analyzer_refresh')} aria-label={$_('analyzer_refresh')}>
				<RefreshCw size="1rem"/>
			</button>
		</div>
	</div>

	{#if analysis}
		<div class="legend">
			{#each analysis.tags as group (group.tag)}
				<button type="button" class="chip" class:active={zoomTag === group.tag} title={formatSize(Number(group.size), $locale)} on:click={() => zoomInto(group.tag)}>
					<span class="swatch" style="background: {getTagColor(group.tag) ?? 'var(--bg-content)'}"></span>
					<span>{tagLabel(group.tag)}</span>
					<span class="share">{shareLabel(Number(group.size), Number(analysis.totalSize), $locale)}</span>
				</button>
			{/each}
		</div>
	{/if}

	<main>
		<div bind:this={sizeRef} id="size-ref"></div>

		<div id="popper" bind:this={popper}></div>

		{#await loading}
			<div id="loading">
				<Loading size="2rem"/>
				{#if transaction}
					<div id="progress">
						{transaction.progress.toFixed(2)}%
						<div id="bar" style="width:calc({transaction.progress}% - .6rem)"></div>
					</div>
					<div id="progress-log" bind:this={progressLog}></div>
				{:else}
					<div id="progress">0.00%</div>
				{/if}
			</div>
		{:then}
			<canvas bind:this={bgCanvas}></canvas>
			<canvas bind:this={addonsCanvas}></canvas>
			<canvas bind:this={tagsCanvas}
				tabindex="0"
				aria-label={$_('size_analyzer')}
				on:mousemove={canvasHover}
				on:mouseleave={() => focusedTile ? showTooltip(focusedTile) : hideTooltip()}
				on:click={canvasClicked}
				on:keydown={canvasKeydown}
				on:focus={() => drawTags(activeTile?.tag ?? focusedTile?.tag ?? null)}
				on:blur={() => drawTags(activeTile?.tag ?? null)}
			></canvas>
		{:catch error}
			<Dead size="2rem"/>
			<div id="error">{$_(typeof error === 'string' ? error : 'ERR_NO_ADDONS_FOUND')}</div>
		{/await}
	</main>
</section>

<style>
	section {
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}

	.toolbar {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: .75rem;
		padding: 1rem 1.5rem 0;
	}
	.heading {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: .4rem;
		margin-inline-end: auto;
		min-width: 0;
	}
	.controls {
		display: flex;
		align-items: center;
		gap: .5rem;
	}
	.toolbar button {
		display: inline-flex;
		align-items: center;
		gap: .4rem;
		border: 1px solid var(--border-strong);
		border-radius: 4px;
		padding: .6rem .75rem;
		background: var(--control);
		color: var(--text);
		font: inherit;
		cursor: pointer;
	}
	.toolbar button:hover {
		background: var(--control-hover);
	}
	.toolbar button.plain {
		margin-inline-start: -.75rem;
		border-color: transparent;
		background: none;
	}
	.toolbar button.plain:hover {
		background: var(--control);
	}
	.search {
		display: flex;
		align-items: center;
		gap: .5rem;
		padding-inline-start: .6rem;
		border: 1px solid var(--border-strong);
		border-radius: 4px;
		background: var(--control);
		color: var(--text-muted);
	}
	.search input {
		min-width: 12rem;
		padding: .6rem .75rem .6rem 0;
		border: 0;
		background: none;
		color: var(--text);
		font: inherit;
	}
	.search input:focus-visible {
		outline: none;
	}
	.search:focus-within {
		outline: 2px solid var(--text);
		outline-offset: 2px;
	}
	.folders {
		display: flex;
		align-items: center;
		gap: .5rem;
		padding-inline: .25rem;
		cursor: pointer;
	}
	.toolbar button:focus-visible, .folders input:focus-visible {
		outline: 2px solid var(--text);
		outline-offset: 2px;
	}

	.total {
		font-weight: 600;
	}
	.count {
		color: var(--text-muted);
	}

	.legend {
		display: flex;
		flex-wrap: wrap;
		gap: .25rem .5rem;
		padding: .5rem 1.5rem 0;
	}
	.chip {
		display: inline-flex;
		align-items: center;
		gap: .4rem;
		border: 1px solid transparent;
		border-radius: 4px;
		padding: .2rem .45rem;
		background: none;
		color: var(--text);
		font: inherit;
		font-size: .85em;
		cursor: pointer;
	}
	.chip:hover {
		background: var(--control);
	}
	.chip.active {
		border-color: var(--neutral);
		background: var(--bg-panel);
	}
	.chip:focus-visible {
		outline: 2px solid var(--text);
		outline-offset: 2px;
	}
	.swatch {
		width: .7em;
		height: .7em;
		border-radius: 2px;
		box-shadow: inset 0 0 0 1px rgba(0, 0, 0, .25);
	}
	.share {
		color: var(--text-muted);
	}

	/* Matches the canvases exactly, so tile coordinates are CSS pixels. */
	#size-ref {
		position: absolute;
		grid-row: 1 / 2;
		grid-column: 1 / 2;
		width: 100%;
		height: 100%;
		pointer-events: none;
		-webkit-pointer-events: none;
		opacity: 0;
	}

	main {
		background-color: var(--bg-app);
		border-radius: .3rem;
		box-shadow: 0 0 0 #000 inset;

		flex: 1;
		min-height: 0;
		width: 100%;
		padding: 1.5rem;

		display: grid;
		grid-template-columns: 100%;
		grid-template-rows: 100%;

		position: relative;
	}
	main > canvas {
		grid-row: 1;
		grid-column: 1;
		width: 100%;
		height: 100%;
		cursor: pointer;
	}
	main > canvas:focus-visible {
		outline: 2px solid #127cff;
		outline-offset: -2px;
	}

	#popper {
		position: absolute;
		width: 0;
		height: 0;
		left: 0;
		top: 0;
		opacity: 0;
		pointer-events: none;
		-webkit-pointer-events: none;

		margin-left: 1.5rem;
		margin-top: 1.5rem;

		transition: opacity .12s, width .12s, height .12s, top .12s, left .12s;
	}
	#popper-content {
		display: none;
	}
	:global(.popper-content) {
		text-align: left;
		display: grid;
		grid-gap: .5rem;
		max-width: 100%;
		word-break: break-word;
		width: 100%;
		min-width: 0;
		grid-template-columns: max-content 1fr;
	}
	:global(.popper-content .row) {
		display: contents;
	}
	:global(.popper-content .row[hidden]) {
		display: none;
	}
	:global(.popper-content .path) {
		font-size: .85em;
		opacity: .8;
	}

	#loading {
		grid-row: 1;
		grid-column: 1;
		width: 100%;
		height: 100%;
		display: flex;
		flex-direction: column;
		justify-content: center;
		align-items: center;
	}
	:global(#loading > .loading) { margin: 0 !important }
	#progress {
		display: inline-block;
		position: relative;
		width: 15%;
		min-width: 250px;
		max-width: calc(100% - 2rem);
		background-color: rgba(0,0,0,.4);
		z-index: 1;
		color: #fff;
		text-align: center;
		text-shadow: 0px 1px 0px rgba(0, 0, 0, 0.6);
		padding: .5rem;

		margin-top: .8rem;
		margin-bottom: .8rem;
	}
	#progress > #bar {
		position: absolute;
		z-index: -1;
		height: calc(100% - .6rem);
		background-color: var(--neutral);
		color: #fff;
		top: .3rem;
		left: .3rem;
	}

	#progress-log {
		text-shadow: var(--text-shadow);
		text-align: center;
		max-height: 0;
	}
	#progress-log :global(.log:not(:last-child)) {
		margin-bottom: .5rem;
	}
	#progress-log :global(.log.stale) {
		animation: stale 2.5s;
		animation-fill-mode: forwards;
	}

	@keyframes stale {
		from { opacity: 1 }
		to { opacity: 0 }
	}
</style>
