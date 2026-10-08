let grammar;

export function highlightLua(source) {
	grammar ??= Prism.languages.extend('lua', {
		comment: [Prism.languages.lua.comment, { pattern: /\/\/[^\r\n]*|\/\*[\s\S]*?(?:\*\/|$)/, greedy: true }],
		keyword: [/\bcontinue\b/, Prism.languages.lua.keyword],
		operator: [/!=|&&|\|\||!/, ...Prism.languages.lua.operator],
	});
	const parts = [];
	function append(tokens, kind = '') {
		for (const token of tokens) {
			if (typeof token === 'string') parts.push({ text: token, kind });
			else append(Array.isArray(token.content) ? token.content : [token.content], token.type);
		}
	}
	append(Prism.tokenize(source, grammar));
	return parts;
}
