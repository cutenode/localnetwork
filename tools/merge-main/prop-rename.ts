// Every #[hegel::test] function is named prop_*: the Hegel runs pick the property tests by that
// prefix, and tools/ast-grep/rules/hegel-test-name.yml refuses a commit with one named otherwise. A
// branch from before that still adds tests by their bare names, so this renames each one to
// prop_<name>, with what names it: calls, `use` lines, doc links, comments and test filters.
//
// In the crate that defines a test (the nearest folder above it with a Cargo.toml), every word that
// is exactly its old name becomes the new one, in the crate's .rs, .md, .toml and .yml files.
// Elsewhere only a mention in backticks is renamed (`name`, [`name`], `module::name`), and only
// when nothing else in the repository has that name.
//
// Before anything is written it looks for other items with a test's name (a plain #[test] in
// another module, a helper, a field, a binding). If one is in the test's crate, the name is renamed
// only in the files that define the test; if one is in such a file, it refuses, naming where. A
// name that is one plain word (`properties`) is renamed in comments and docs only in backticks, so
// prose using that word is left alone.
import { posix } from 'node:path';

/** The rule tools/ast-grep/rules/hegel-test-name.yml checks, without its name test, so it finds every hegel test. */
export const HEGEL_TESTS = `
id: hegel-test
language: rust
rule:
  kind: function_item
  follows:
    stopBy: { not: { any: [{ kind: attribute_item }, { kind: line_comment }, { kind: block_comment }] } }
    kind: attribute_item
    regex: '^#\\[hegel::test[(\\]]'
`;

/** A hegel test: its file, its function's name, and the line its match starts on. */
interface HegelTest {
	file: string;
	name: string;
	line: number;
}

/** The tests in `ast-grep scan --json=stream`'s output, one match per line. */
export const parseScan = (stream: string): HegelTest[] =>
	stream
		.split('\n')
		.filter(Boolean)
		.map((line) => {
			// SAFETY: ast-grep's JSON stream, one match per line with its file, text and range.
			const { file, text, range } = JSON.parse(line) as {
				file: string;
				text: string;
				range: { start: { line: number } };
			};

			const name = /\bfn\s+(?:r#)?([A-Za-z_]\w*)/.exec(text)?.[1];

			if (!name) throw new Error(`no fn name in ${file}:${range.start.line + 1}`);

			return { file, name, line: range.start.line + 1 };
		});

/** The files whose words can name a test. */
export const TEXT = /\.(rs|md|toml|ya?ml)$/;

const WORD = /(?<![\w#])(r#)?([A-Za-z_]\w*)/g;

const BACKTICKED = /`((?:\w+::)*)([A-Za-z_]\w*)(\(\))?`/g;

const DEFINITION =
	/\b(?:fn|let(?:\s+mut)?|struct|enum|mod|const|static|type|trait|macro_rules!)\s+(?:r#)?([A-Za-z_]\w*)|(?<![\w.:])([A-Za-z_]\w*)\s*:(?!:)/g;

const isCommentLine = (line: string) => /^\s*(\/\/|\*)/.test(line);

/**
 * The texts that change when the tests not yet named prop_* are, among `texts` (path -> text), with
 * `crates` the folders that have a Cargo.toml ('.' for the root). `collisions` names what has to be
 * renamed by hand first, and nothing changes while there are any; `notes` says where a name was
 * renamed only in its test's files, and `leftover` lists bare old names outside their crates, for a look.
 */
export const propRename = (tests: HegelTest[], texts: Map<string, string>, crates: Set<string>) => {
	const crateOf = (file: string) => {
		let dir = posix.dirname(file);

		while (!crates.has(dir) && dir !== '.') dir = posix.dirname(dir);

		return crates.has(dir) ? dir : null;
	};

	const toRename = tests.filter(({ name }) => !name.startsWith('prop_'));
	const testLines = new Set(tests.map(({ file, line }) => `${file}:${line}`));

	// crate -> old name -> the files defining a test of that name
	const renames = new Map<string | null, Map<string, Set<string>>>();

	for (const { file, name } of toRename) {
		const names = renames.get(crateOf(file)) ?? new Map<string, Set<string>>();

		renames.set(crateOf(file), names.set(name, (names.get(name) ?? new Set()).add(file)));
	}

	const allNames = new Set(toRename.map(({ name }) => name));

	// Other items with a test's name, anywhere: name -> where.
	const others = new Map<string, { file: string; where: string }[]>();
	const otherLines = new Set<string>();

	for (const [file, text] of texts) {
		if (!file.endsWith('.rs')) continue;

		for (const [i, line] of text.split('\n').entries()) {
			if (isCommentLine(line) || testLines.has(`${file}:${i + 1}`)) continue;

			for (const m of line.matchAll(DEFINITION)) {
				const name = m[1] ?? m[2];

				if (!allNames.has(name)) continue;
				others.set(name, [...(others.get(name) ?? []), { file, where: `${file}:${i + 1}: ${line.trim()}` }]);
				otherLines.add(`${file}:${i + 1}`);
			}
		}
	}

	// crate -> name -> the only files to rename it in
	const narrowed = new Map<string | null, Map<string, Set<string>>>();
	const collisions: string[] = [];
	const notes: string[] = [];

	for (const [crate, names] of renames) {
		for (const [name, defining] of names) {
			for (const { file, where } of others.get(name) ?? []) {
				if (crateOf(file) !== crate) continue;

				if (defining.has(file)) {
					collisions.push(where);
					continue;
				}

				narrowed.set(crate, (narrowed.get(crate) ?? new Map<string, Set<string>>()).set(name, defining));
				notes.push(`${name}: also ${where}, so renamed only in ${[...defining].join(', ')}`);
			}
		}
	}

	const changed = new Map<string, string>();
	const leftover = new Set<string>();

	if (collisions.length > 0) return { changed, collisions, notes, leftover: [] };

	const renameIn = (file: string, text: string) => {
		const crate = crateOf(file);
		const own = renames.get(crate);
		const ownName = (name: string) => Boolean(own?.has(name) && (narrowed.get(crate)?.get(name)?.has(file) ?? true));

		const renameWords = (line: string, which: (name: string) => boolean) =>
			line.replace(WORD, (whole, raw: string | undefined, name: string) =>
				which(name) ? `${raw ?? ''}prop_${name}` : whole
			);

		// Renamed in backticks outside its crate: a name no other item has.
		const mentioned = (line: string) =>
			line.replace(BACKTICKED, (whole, path: string, name: string, call: string | undefined) => {
				const renamed = ownName(name) || (allNames.has(name) && !own?.has(name) && !others.has(name));

				return renamed ? `\`${path}prop_${name}${call ?? ''}\`` : whole;
			});

		const isProse = (line: string) => !file.endsWith('.rs') || isCommentLine(line);

		return text
			.split('\n')
			.map((line) =>
				isProse(line)
					? // One plain word is renamed in prose only in backticks, as `mentioned` does.
						renameWords(mentioned(line), (name) => name.includes('_') && ownName(name))
					: renameWords(mentioned(line), ownName)
			)
			.join('\n');
	};

	for (const [file, text] of texts) {
		const after = renameIn(file, text);

		if (after !== text) changed.set(file, after);

		// Bare mentions the rules above don't reach, outside the defining crate.
		for (const [i, line] of after.split('\n').entries()) {
			if (otherLines.has(`${file}:${i + 1}`)) continue;

			for (const m of line.matchAll(WORD)) {
				if (allNames.has(m[2]) && m[2].includes('_') && !renames.get(crateOf(file))?.has(m[2]))
					leftover.add(`${file}:${i + 1}: ${line.trim()}`);
			}
		}
	}

	return { changed, collisions, notes, leftover: [...leftover] };
};
