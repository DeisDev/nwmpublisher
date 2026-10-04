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
	return root.children;
}
