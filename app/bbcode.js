const supportedTags = new Set(['b', 'i', 'u', 'strike', 'h1', 'h2', 'h3', 'spoiler', 'url', 'list', 'olist', 'quote', 'code', 'noparse', 'img', 'hr']);
const literalTags = new Set(['code', 'noparse', 'img']);

export function parseBBCode(source) {
	const root = { children: [] };
	const stack = [root];
	const tokens = /\[(\/?)([a-z][a-z0-9]*|\*)(?:=([^\]\r\n]*))?\]/gi;
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
		const [opening, closing, name, argument] = match;
		const tag = name.toLowerCase();

		if (tag === '*' && !closing && argument === undefined) {
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

		if (!supportedTags.has(tag) || (argument !== undefined && (closing || !['url', 'quote'].includes(tag)))) {
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
		stack[stack.length - 1].children.push(node);
		stack.push(node);
	}

	append(source.slice(position));
	while (stack.length > 1) preserveUnclosed();
	return root.children;
}
