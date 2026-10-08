const supportedTags = new Set(['b', 'i', 'u', 'strike', 'h1', 'h2', 'h3', 'spoiler', 'url', 'list', 'olist', 'quote', 'code', 'noparse', 'img', 'hr', 'table', 'tr', 'th', 'td']);
const literalTags = new Set(['code', 'noparse', 'img']);
const argumentTags = new Set(['url', 'quote']);
const toggleTags = new Set(['b', 'i', 'u', 'strike', 'h1', 'h2', 'h3', 'spoiler', 'url', 'quote', 'code', 'noparse']);
const listItem = /^([ \t]*)\[\*\]/;
const listOpening = /^([ \t]*).*\[o?list\][ \t]*$/i;
const listClosing = /^[ \t]*\[\/o?list\]/i;
const listClosingBelow = /^[ \t]*\n?([ \t]*\[\/o?list\])/i;

export function listBreak(value, caret) {
	const lineStart = value.lastIndexOf('\n', caret - 1) + 1;
	const newline = value.indexOf('\n', caret);
	const lineEnd = newline === -1 ? value.length : newline;
	const line = value.slice(lineStart, caret);
	const after = value.slice(caret, lineEnd);
	const suffix = listClosing.test(after) ? '\n' : '';
	const item = listItem.exec(line);
	if (item) {
		const closing = listClosingBelow.exec(value.slice(caret));
		if (line.slice(item[0].length).trim() || (!closing && after.trim())) return { marker: item[1] + '[*]', suffix };
		if (closing) return { start: lineStart, end: caret + closing[0].length, text: closing[1] + '\n' };
		return { start: lineStart, end: lineEnd, text: '' };
	}
	const opening = listOpening.exec(line);
	if (opening && (suffix || !after.trim())) return { marker: opening[1] + '[*]', suffix };
	return null;
}

function tagPatterns(tag) {
	return [`\\[${tag}${argumentTags.has(tag) ? '(?:=[^\\]\\r\\n]*)?' : ''}\\]`, `\\[/${tag}\\]`];
}

function balanced(text, opening, closing) {
	let depth = 0;
	for (const [, close] of text.matchAll(new RegExp(`${opening}|(${closing})`, 'gi'))) {
		depth += close ? -1 : 1;
		if (depth < 0) return false;
	}
	return depth === 0;
}

export function unwrapTag(value, start, end, tag) {
	if (!toggleTags.has(tag)) return null;
	const [opening, closing] = tagPatterns(tag);
	const selected = value.slice(start, end);
	const wrapped = new RegExp(`^${opening}([\\s\\S]*)${closing}$`, 'i').exec(selected);
	if (wrapped && balanced(wrapped[1], opening, closing)) return { start, end, text: wrapped[1] };
	const before = new RegExp(`${opening}$`, 'i').exec(value.slice(0, start));
	const after = new RegExp(`^${closing}`, 'i').exec(value.slice(end));
	if (before && after && balanced(selected, opening, closing)) return { start: before.index, end: end + after[0].length, text: selected };
	return null;
}

export function closingTag(value, caret) {
	const open = value.lastIndexOf('[', caret - 1);
	const match = open === -1 ? null : /^\[([a-z][a-z0-9]*)(=[^\]\r\n]*)?$/i.exec(value.slice(open, caret));
	const tag = match?.[1].toLowerCase();
	if (!match || tag === 'hr' || !supportedTags.has(tag) || (match[2] !== undefined && !argumentTags.has(tag))) return null;
	const closing = `[/${match[1]}]`;
	return value.slice(caret, caret + closing.length).toLowerCase() === closing.toLowerCase() ? null : closing;
}

export function linkText(text) {
	const link = text.trim();
	if (!/^https?:\/\/[^\s\]]+$/i.test(link)) return null;
	try {
		new URL(link);
	} catch {
		return null;
	}
	return link;
}

export function bbcodeUrl(source) {
	let value = source.trim();
	if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) value = value.slice(1, -1);
	if (!value || /[\s\\\u0000-\u001f\u007f]/u.test(value)) return null;
	if (/^[\p{L}\p{N}.-]+\.[\p{L}\p{N}-]+(?::\d+)?(?:[/?#]|$)/u.test(value)) value = 'https://' + value;
	try {
		const url = new URL(value);
		return ['https:', 'http:'].includes(url.protocol) && !url.username && !url.password ? url.href : null;
	} catch (error) {
		if (error instanceof TypeError) return null;
		throw error;
	}
}

export function bbcodeWidget(href) {
	const value = bbcodeUrl(href);
	if (!value) return null;
	const url = new URL(value);
	if (url.port) return null;
	if (['youtube.com', 'www.youtube.com', 'm.youtube.com', 'youtu.be'].includes(url.hostname)) {
		const id = url.hostname === 'youtu.be' ? url.pathname.slice(1) : url.pathname === '/watch' ? url.searchParams.get('v') : null;
		if (!id || !/^[\w-]{11}$/.test(id)) return null;
		const start = url.searchParams.get('start') ?? url.searchParams.get('t');
		const seconds = start && /^\d+$/.test(start) ? Number(start) : 0;
		return { type: 'youtube', src: 'https://www.youtube.com/embed/' + id + (seconds ? '?start=' + seconds : '') };
	}
	if (url.hostname === 'store.steampowered.com') {
		const id = /^\/app\/([1-9]\d*)(?:\/|$)/.exec(url.pathname)?.[1];
		if (id) return { type: 'store', src: 'https://store.steampowered.com/widget/' + id + '/' };
	}
	if (url.hostname === 'steamcommunity.com' && /^\/(?:sharedfiles|workshop)\/filedetails\/?$/.test(url.pathname)) {
		const ids = url.searchParams.getAll('id');
		const id = ids.length === 1 ? ids[0] : null;
		if (id && /^[1-9]\d{0,19}$/.test(id) && BigInt(id) <= 18446744073709551615n) return { type: 'workshop', id };
	}
	return null;
}

function nodeText(nodes) {
	return nodes.map(node => typeof node === 'string' ? node : node.text ?? nodeText(node.children ?? [])).join('');
}

function linkify(nodes, insideLink = false) {
	return nodes.flatMap(node => {
		if (typeof node !== 'string') {
			const link = node.tag === 'url';
			if (link) node.href = insideLink ? null : bbcodeUrl(node.argument ?? nodeText(node.children));
			if (node.children) node.children = linkify(node.children, insideLink || link);
			return [node];
		}
		if (insideLink) return [node];
		const parts = [];
		let position = 0;
		for (const match of node.matchAll(/\b(?:https?:\/\/|www\.)[^\s<>\[\]"']+/gi)) {
			let text = match[0].replace(/[.,!?;:]+$/, '');
			while (text.endsWith(')') && text.split(')').length > text.split('(').length) text = text.slice(0, -1);
			const href = bbcodeUrl(text);
			if (!href) continue;
			if (match.index > position) parts.push(node.slice(position, match.index));
			parts.push({ tag: 'url', href, automatic: true, children: [text] });
			position = match.index + text.length;
		}
		if (position < node.length) parts.push(node.slice(position));
		return parts;
	});
}

export function parseBBCode(source) {
	const root = { children: [] };
	const stack = [root];
	const tokens = /\[(\/?)([a-z][a-z0-9]*|\*)(?:=([^\]\r\n]*)|([ \t]+[^\]\r\n]*))?\]/gi;
	let position = 0;
	let match;

	function append(text) {
		if (!text) return;
		const children = stack[stack.length - 1].children;
		if (typeof children[children.length - 1] === 'string') children[children.length - 1] += text;
		else children.push(text);
	}

	function preserveUnclosed() {
		const node = stack.pop();
		if (node.tag === 'item') {
			const last = node.children.length - 1;
			if (typeof node.children[last] === 'string') node.children[last] = node.children[last].replace(/\r?\n[\t ]*$/, '');
		} else {
			node.children.unshift(node.opening);
			node.tag = null;
		}
	}

	while ((match = tokens.exec(source))) {
		append(source.slice(position, match.index));
		position = tokens.lastIndex;
		const [opening, closing, name, argument, attributes] = match;
		const tag = name.toLowerCase();
		const options = attributes?.trim().toLowerCase().split(/[ \t]+/) ?? [];

		if (tag === '*' && !closing && argument === undefined && attributes === undefined) {
			let listIndex = stack.length - 1;
			while (listIndex > 0 && !['list', 'olist'].includes(stack[listIndex].tag)) listIndex--;
			if (listIndex === 0) {
				append(opening);
				continue;
			}
			while (stack.length - 1 > listIndex) preserveUnclosed();
			const item = { tag: 'item', children: [] };
			stack[listIndex].children.push(item);
			stack.push(item);
			continue;
		}

		if (!supportedTags.has(tag)
			|| (argument !== undefined && (closing || !['url', 'quote'].includes(tag)))
			|| (attributes !== undefined && (closing || tag !== 'table' || options.some(option => !['noborder=1', 'equalcells=1'].includes(option))))) {
			append(opening);
			continue;
		}

		const parent = stack[stack.length - 1].tag;
		if (!closing && ((tag === 'tr' && parent !== 'table') || (['th', 'td'].includes(tag) && parent !== 'tr'))) {
			append(opening);
			continue;
		}

		if (closing) {
			if (['list', 'olist'].includes(tag) && stack[stack.length - 1].tag === 'item' && stack[stack.length - 2]?.tag === tag) preserveUnclosed();
			if (stack.length > 1 && stack[stack.length - 1].tag === tag) stack.pop();
			else append(opening);
			continue;
		}

		if (literalTags.has(tag)) {
			const endTag = new RegExp(`\\[/${tag}\\]`, 'gi');
			endTag.lastIndex = position;
			const end = endTag.exec(source);
			if (!end) {
				append(source.slice(match.index));
				position = source.length;
				break;
			}
			stack[stack.length - 1].children.push({ tag, text: source.slice(position, end.index) });
			position = tokens.lastIndex = endTag.lastIndex;
			continue;
		}

		if (tag === 'hr') {
			stack[stack.length - 1].children.push({ tag });
			if (source.slice(position, position + 5).toLowerCase() === '[/hr]') position = tokens.lastIndex += 5;
			continue;
		}

		// Bound rendering depth for pasted descriptions with excessive nesting.
		if (stack.length >= 32) {
			append(opening);
			continue;
		}
		const node = { tag, argument, opening, children: [] };
		if (tag === 'table') {
			node.noborder = options.includes('noborder=1');
			node.equalcells = options.includes('equalcells=1');
		}
		stack[stack.length - 1].children.push(node);
		stack.push(node);
	}

	append(source.slice(position));
	while (stack.length > 1) preserveUnclosed();
	return linkify(root.children);
}

const keepAChangelogHeading = /^(?:##[ \t]+\[?(?:unreleased|v?\d+\.\d+\.\d+)|###[ \t]+(?:added|changed|deprecated|removed|fixed|security)[ \t]*$)/im;
const changelogSectionHeading = /^[ \t]{0,3}##[ \t]/;
const unreleasedHeading = /^[ \t]{0,3}##[ \t]+\[?unreleased\]?/i;
const bbcodeTag = /\[\/?(?:b|i|u|strike|h[1-3]|spoiler|url|list|olist|quote|code|noparse|img|hr|table|tr|th|td)(?:=[^\]\n]*)?\]|\[\*\]/i;
const markdownSignals = [
	/^[ \t]{0,3}#{1,6}[ \t]+\S/m,
	/^[ \t]{0,3}(?:`{3,}|~{3,})/m,
	/!?\[[^\]\n]+\]\([^)\s]+\)/,
	/\*\*\S(?:[^\n]*?\S)?\*\*/,
	/^[ \t]*(?:[-*+]|\d{1,9}[.)])[ \t]+\S.*\n[ \t]*(?:[-*+]|\d{1,9}[.)])[ \t]+\S/m,
];
const markdownHeading = /^[ \t]{0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;
const markdownItem = /^([ \t]*)(?:([-*+])|\d{1,9}[.)])[ \t]+(.*)$/;
const markdownRule = /^[ \t]{0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const markdownFence = /^[ \t]{0,3}(`{3,}|~{3,})/;
const markdownReference = /^[ \t]{0,3}\[([^\]]+)\]:[ \t]*<?([^\s>]+)>?(?:[ \t]+(?:"[^"]*"|'[^']*'|\([^)]*\)))?[ \t]*$/;
const markdownInlineToken = /(`+)(?!`)([\s\S]*?[^`])\1(?!`)|(!?)\[([^\]]*)\](?:\(<?([^\s()<>]+)>?(?:[ \t]+(?:"[^"]*"|'[^']*'))?\)|\[([^\]]*)\])?|<(https?:\/\/[^\s<>]+)>|https?:\/\/[^\s<>()[\]]+|\\([!-/:-@[-`{-~])/g;

function referenceKey(label) {
	return label.trim().replace(/\s+/g, ' ').toLowerCase();
}

function markdownInline(text, references) {
	const kept = [];
	// Finished BBCode and literal text are held in placeholders so emphasis rules cannot alter them.
	const keep = value => `\uE000${kept.push(value) - 1}\uE001`;
	return text
		.replace(markdownInlineToken, (match, ticks, code, image, label, url, reference, autolink, escaped) => {
			if (ticks) {
				const content = code.trim();
				return keep(content.includes('[') ? `[noparse]${content}[/noparse]` : content);
			}
			if (escaped) return keep(escaped);
			if (autolink) return keep(autolink);
			if (label === undefined) return keep(match);
			const target = url ?? references.get(referenceKey(reference || label));
			if (!target) return match;
			if (image) return keep(`[img]${target}[/img]`);
			return keep(`[url=${target.replace(/]/g, '%5D')}]${markdownInline(label, references)}[/url]`);
		})
		.replace(/\*\*(?=\S)([\s\S]*?\S)\*\*/g, '[b]$1[/b]')
		.replace(/(^|[^\p{L}\p{N}_])__(?=\S)([\s\S]*?\S)__(?![\p{L}\p{N}_])/gu, '$1[b]$2[/b]')
		.replace(/~~(?=\S)([\s\S]*?\S)~~/g, '[strike]$1[/strike]')
		.replace(/\*(?=[^\s*])([^*]*?[^\s*])\*/g, '[i]$1[/i]')
		.replace(/(^|[^\p{L}\p{N}_])_(?=[^\s_])([^_]*?[^\s_])_(?![\p{L}\p{N}_])/gu, '$1[i]$2[/i]')
		.replace(/\uE000(\d+)\uE001/g, (_, index) => kept[index]);
}

function renderList(items, references) {
	const lines = [];
	const open = [];
	for (const item of items) {
		while (open.length && (item.indent < open[open.length - 1].indent || (item.indent === open[open.length - 1].indent && item.tag !== open[open.length - 1].tag))) {
			lines.push(`[/${open.pop().tag}]`);
		}
		if (!open.length || item.indent > open[open.length - 1].indent) {
			open.push(item);
			lines.push(`[${item.tag}]`);
		}
		lines.push('[*]' + markdownInline(item.text, references));
	}
	while (open.length) lines.push(`[/${open.pop().tag}]`);
	return lines.join('\n');
}

export function markdownToBBCode(source) {
	const text = source.replace(/\r\n?/g, '\n');
	const references = new Map();
	const lines = text.split('\n').filter(line => {
		const reference = markdownReference.exec(line);
		const key = reference && referenceKey(reference[1]);
		if (reference && !references.has(key)) references.set(key, reference[2]);
		return !reference;
	});

	const blocks = [];
	let block = null;
	for (let index = 0; index < lines.length; index++) {
		const line = lines[index];
		const fence = markdownFence.exec(line);
		if (fence) {
			const closing = new RegExp(`^[ \\t]{0,3}${fence[1][0]}{${fence[1].length},}[ \\t]*$`);
			let end = index + 1;
			while (end < lines.length && !closing.test(lines[end])) end++;
			blocks.push({ type: 'code', text: `[code]${lines.slice(index + 1, end).join('\n')}[/code]` });
			block = null;
			index = end;
			continue;
		}
		const heading = markdownHeading.exec(line);
		if (heading) {
			const level = Math.min(heading[1].length, 3);
			// Keep a Changelog links version headings to definitions that a pasted section often leaves out.
			const title = (heading[2] ?? '').replace(/^\[([^\]]+)\](?![[(])/, (match, label) => references.has(referenceKey(label)) ? match : label);
			blocks.push({ type: 'heading', text: `[h${level}]${markdownInline(title, references)}[/h${level}]` });
			block = null;
			continue;
		}
		if (markdownRule.test(line)) {
			blocks.push({ type: 'rule', text: '[hr][/hr]' });
			block = null;
			continue;
		}
		if (!line.trim()) {
			if (block?.type === 'list') block.blank = true;
			else block = null;
			continue;
		}
		const item = markdownItem.exec(line);
		if (item) {
			if (block?.type !== 'list') blocks.push(block = { type: 'list', items: [] });
			block.blank = false;
			block.items.push({ indent: item[1].replace(/\t/g, '    ').length, tag: item[2] ? 'list' : 'olist', text: item[3].trim() });
			continue;
		}
		if (block?.type === 'list' && (!block.blank || /^[ \t]/.test(line))) {
			const last = block.items[block.items.length - 1];
			last.text = [last.text, line.trim()].filter(Boolean).join(' ');
			block.blank = false;
			continue;
		}
		if (block?.type === 'paragraph') block.lines.push(line.trim());
		else blocks.push(block = { type: 'paragraph', lines: [line.trim()] });
	}

	let output = '';
	blocks.forEach((block, index) => {
		if (index) output += block.type === 'paragraph' && blocks[index - 1].type === 'paragraph' ? '\n\n' : '\n';
		if (block.type === 'list') output += renderList(block.items, references);
		else if (block.type === 'paragraph') output += markdownInline(block.lines.join(' '), references);
		else output += block.text;
	});
	return text.endsWith('\n') ? output + '\n' : output;
}

export function changelogToBBCode(source) {
	const text = source.replace(/\r\n?/g, '\n');
	return keepAChangelogHeading.test(text) && !bbcodeTag.test(text) ? markdownToBBCode(text) : null;
}

export function markdownPasteToBBCode(source) {
	const text = source.replace(/\r\n?/g, '\n');
	return markdownSignals.some(signal => signal.test(text)) && !bbcodeTag.test(text) ? markdownToBBCode(text) : null;
}

export function changelogImportSection(source) {
	const lines = source.replace(/\r\n?/g, '\n').split('\n');
	const references = lines.filter(line => markdownReference.test(line));
	const hasChanges = section => section?.body.some(line => line.trim());
	let section = null;
	for (const line of lines) {
		if (changelogSectionHeading.test(line)) {
			if (hasChanges(section)) break;
			section = { heading: line, body: [] };
		} else if (section && !markdownReference.test(line)) {
			section.body.push(line);
		}
	}
	if (!hasChanges(section)) return null;
	const heading = unreleasedHeading.test(section.heading) ? [] : [section.heading];
	return [...heading, ...section.body, '', ...references].join('\n').trim();
}

function* sourceTags(value) {
	const pattern = /\[(\/?)([a-z][a-z0-9]*|\*)(?:(=[^\]\r\n]*)|([ \t]+[^\]\r\n]*))?\]/gi;
	let literal = null;
	for (const match of value.matchAll(pattern)) {
		const [, closing, name, argument, attributes] = match;
		const tag = name.toLowerCase();
		if (literal && !(closing && tag === literal && !argument && !attributes)) continue;
		if (!supportedTags.has(tag) && tag !== '*') continue;
		if (tag === '*' && (closing || argument || attributes)) continue;
		if (argument && (closing || !argumentTags.has(tag))) continue;
		if (attributes && (closing || tag !== 'table' || attributes.trim().toLowerCase().split(/[ \t]+/).some(option => !['noborder=1', 'equalcells=1'].includes(option)))) continue;
		yield { text: match[0], start: match.index, tag, closing: !!closing, argument: argument ?? attributes };
		if (literal) literal = null;
		else if (!closing && literalTags.has(tag)) literal = tag;
	}
}

export function highlightBBCode(value) {
	const parts = [];
	let position = 0;
	for (const token of sourceTags(value)) {
		if (token.start > position) parts.push({ text: value.slice(position, token.start), kind: '' });
		if (token.argument) {
			const split = token.text.length - token.argument.length - 1;
			parts.push({ text: token.text.slice(0, split), kind: 'tag' }, { text: token.argument, kind: 'string' }, { text: ']', kind: 'tag' });
		} else parts.push({ text: token.text, kind: 'tag' });
		position = token.start + token.text.length;
	}
	if (position < value.length) parts.push({ text: value.slice(position), kind: '' });
	return parts;
}

export function tagCompletions(value, caret) {
	const prefix = /\[(\/?)([a-z0-9*]*)$/i.exec(value.slice(0, caret));
	if (!prefix) return null;
	const open = [];
	for (const token of sourceTags(value.slice(0, prefix.index))) {
		if (token.closing) {
			const index = open.lastIndexOf(token.tag);
			if (index !== -1) open.length = index;
		} else if (!['*', 'hr'].includes(token.tag)) open.push(token.tag);
	}
	const literal = literalTags.has(open[open.length - 1]);
	if (literal && !prefix[1]) return null;
	const names = prefix[1] ? [...new Set(open.reverse())] : [...supportedTags, '*'];
	const options = names.filter(tag => tag.startsWith(prefix[2].toLowerCase()) && (!literal || tag === open[0]));
	return options.length ? { start: prefix.index, end: caret, closing: !!prefix[1], options } : null;
}

export function completeTag(value, completion, tag, autoClose) {
	let end = completion.end;
	const tail = /^[a-z0-9*]*(=[^\]\r\n]*)?\]/i.exec(value.slice(end));
	if (tail) end += tail[0].length;
	let text = completion.closing ? '[/' + tag + ']' : '[' + tag + ']';
	let offset = text.length;
	if (!completion.closing && argumentTags.has(tag)) {
		text = '[' + tag + (tail?.[1] ?? '=') + ']';
		offset = text.length - 1;
	}
	if (autoClose && !tail && !completion.closing && !['*', 'hr'].includes(tag)) {
		const closing = '[/' + tag + ']';
		if (value.slice(end, end + closing.length).toLowerCase() !== closing) text += closing;
	}
	return { start: completion.start, end, text, caret: completion.start + offset };
}

export function indentLines(value, start, end, outdent) {
	if (start === end && !outdent) return { start, end, text: '\t', selection: [start + 1, start + 1] };
	const from = start === 0 ? 0 : value.lastIndexOf('\n', start - 1) + 1;
	const last = end > start && value[end - 1] === '\n' ? end - 1 : end;
	const newline = value.indexOf('\n', last);
	const to = newline === -1 ? value.length : newline;
	const lines = value.slice(from, to).split('\n');
	const changes = lines.map(line => outdent ? -(line.match(/^(?:\t| {1,4})/)?.[0].length ?? 0) : 1);
	const text = lines.map((line, index) => outdent ? line.slice(-changes[index]) : '\t' + line).join('\n');
	return { start: from, end: to, text, selection: [Math.max(from, start + changes[0]), Math.max(from, end + changes.reduce((sum, change) => sum + change, 0))] };
}
