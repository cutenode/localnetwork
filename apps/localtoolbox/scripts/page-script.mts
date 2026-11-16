// What the build makes of a page script (vite.config.ts, `pageScripts`): the
// TypeScript with its types blanked out and its comments taken away, every
// token left on the line and column it had in the source, so an error on
// soundcloud.com points at the same place in content.ts with no sourcemap to
// ship.
import blankSpace from 'ts-blank-space';
import { parseSync } from 'oxc-parser';

// A licence has to travel with the code it covers.
const isLicence = (comment: string) => comment.startsWith('/*!') || comment.includes('@license');

const isBlank = (char: string | undefined) => char === ' ' || char === '\t';

export function pageScript(source: string) {
	const code = blankSpace(source);
	// One entry per UTF-16 unit, as the parser counts offsets; '' is a unit taken out.
	const chars = code.split('');

	// From the parser rather than a pattern: `//` and `/*` turn up in strings,
	// template literals and regular expressions too.
	for (const { start, end } of parseSync('page-script.js', code).comments) {
		if (isLicence(code.slice(start, end))) continue;

		let after = end;

		while (isBlank(code[after])) after++;
		// LF only: .gitattributes checks every file out that way.
		const endsLine = after === code.length || code[after] === '\n';
		// Code after the comment on its last line keeps its column, so that
		// stretch turns to spaces. The rest goes, newlines aside.
		const spacesFrom = endsLine ? end : Math.max(start, code.lastIndexOf('\n', end - 1) + 1);

		for (let i = start; i < end; i++) {
			if (chars[i] !== '\n') chars[i] = i < spacesFrom ? '' : ' ';
		}

		if (endsLine) chars.fill('', end, after);

		// The comment ran to the end of a line, so the whitespace in front of it
		// goes too. It is never inside a string or template literal: the comment
		// would be as well.
		if (spacesFrom > start) {
			for (let i = start - 1; isBlank(chars[i]) || chars[i] === ''; i--) chars[i] = '';
		}
	}

	return chars.join('');
}
