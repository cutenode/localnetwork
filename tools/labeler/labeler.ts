// Builds the config actions/labeler reads (pr-labels.yml), which labels a pull request from the
// files it changes, so that labelling a new crate, plugin or file adds or edits a file of its own:
//
// - Each crate and plugin names its area labels in its Cargo.toml, as `labels` under
//   [package.metadata.localnetwork], and gets them for every file in its folder.
// - Every other label's globs are in .github/labeler/<label>.yml, the label's entry as
//   actions/labeler's config has it, one glob per line.
// - A tool's catch-all area (`web: ui`, `locallibrary: core`) lists its folders under
//   `unclaimed-files-in`, and gets every file in them that no other area of its tool claims. A new
//   area takes its files from the catch-all without editing it.
//
// A file can get more than one area, and lexicons go with the feature that reads or writes them.
//
// pr-labels.yml runs this under pull_request_target, from main's files and with no dependencies
// installed, so it has only Node's built-ins. It reads the files in the one layout they're written
// in (LINES), not YAML at large, and writes JSON, which actions/labeler reads as YAML.
import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';

/** A per-label file in .github/labeler/: its name, for errors, and its text. */
export interface LabelFile {
	name: string;
	text: string;
}

/** What this reads of a package in `cargo metadata --no-deps --format-version 1`. */
export interface CargoPackage {
	manifest_path: string;
	metadata: { localnetwork?: { labels?: string[] } } | null;
}

/** What this reads of `cargo metadata --no-deps --format-version 1`. */
export interface CargoMetadata {
	workspace_root: string;
	packages: CargoPackage[];
}

/** One actions/labeler match, as `{ "any-glob-to-any-file": [...] }`. */
type Match = Record<string, string[]>;

const UNCLAIMED = 'unclaimed-files-in';

const MATCHES = new Set([
	'any-glob-to-any-file',
	'any-glob-to-all-files',
	'all-globs-to-any-file',
	'all-globs-to-all-files',
	UNCLAIMED
]);

// The layout the files are written in, line by line: the label as the key, `- changed-files:`
// rules, a match's key, and one quoted glob per line, indented so. Comments and blank lines go anywhere.
const LINES = {
	label: /^"([^"]+)":$/,
	rule: /^ {2}- changed-files:$/,
	match: /^ {6}- ([a-z-]+):$/,
	glob: /^ {10}- ("[^"]*")$/
};

const read = ({ name, text }: LabelFile) => {
	let label = '';
	const rules: Match[][] = [];

	for (const [i, line] of text.split('\n').entries()) {
		if (/^\s*(#.*)?$/.test(line)) continue;

		const fail = (why: string) => new Error(`.github/labeler/${name}:${i + 1}: ${why}`);
		const [kind, found] = Object.entries(LINES).find(([, pattern]) => pattern.test(line)) ?? [];
		const value = found?.exec(line)?.[1] ?? '';
		const matches = rules.at(-1);
		const globs = matches?.at(-1);

		if (!kind) throw fail('not a label, `- changed-files:`, a match or a quoted glob, indented as the others');

		if ((kind === 'label') === Boolean(label)) throw fail('one label per file, on its first line');

		if (kind === 'label') label = value;
		else if (kind === 'rule') rules.push([]);
		else if (kind === 'match') {
			if (!matches) throw fail('a match goes under `- changed-files:`');

			if (!MATCHES.has(value)) throw fail(`there's no match called ${value}`);

			matches.push({ [value]: [] });
		} else {
			if (!globs) throw fail('a glob goes under a match');

			Object.values(globs)[0].push(JSON.parse(value));
		}
	}

	if (!label) throw new Error(`.github/labeler/${name}: has no label`);

	return { label, rules };
};

// `{a,b}/x{.rs,/**}` as the globs it stands for.
const expand = (glob: string): string[] => {
	const group = /\{([^{}]*)\}/.exec(glob);

	if (!group) return [glob];

	return group[1]
		.split(',')
		.flatMap((option) => expand(glob.slice(0, group.index) + option + glob.slice(group.index + group[0].length)));
};

// The folder a glob starts in, before its first wildcard: `apps/web/src/` for `apps/web/src/**/x.ts`.
const folderOf = (glob: string) => glob.slice(0, glob.search(/[*?[{]|$/)).replace(/[^/]*$/, '');

// Whether `glob` can match a file in `folder`.
const reaches = (glob: string, folder: string) =>
	expand(glob).some((each) => each.startsWith(folder) || folder.startsWith(folderOf(each)));

const toolOf = (label: string) => label.split(': ')[0];

/** The labeler config for these per-label files and the crates and plugins in `metadata`. */
export const buildConfig = (files: LabelFile[], metadata: CargoMetadata) => {
	const labels = new Map<string, { file?: string; rules: Match[][] }>();

	for (const file of files) {
		const { label, rules } = read(file);
		const other = labels.get(label)?.file;

		if (other) throw new Error(`.github/labeler/${file.name}: ${JSON.stringify(label)} is in ${other} too`);
		labels.set(label, { file: file.name, rules });
	}

	for (const { manifest_path, metadata: own } of metadata.packages) {
		const folder = relative(metadata.workspace_root, join(manifest_path, '..')).replaceAll('\\', '/');

		for (const label of own?.localnetwork?.labels ?? []) {
			const entry = labels.get(label) ?? { rules: [] };
			const crates = entry.rules.find((rule) => rule[0]['crates and plugins']);

			if (crates) crates[0]['crates and plugins'].push(`${folder}/**`);
			else entry.rules.push([{ 'crates and plugins': [`${folder}/**`] }]);
			labels.set(label, entry);
		}
	}

	// A catch-all's folder less the globs the other areas of its tool claim files with (all but the
	// negated ones). Each tool has one catch-all, so the others' globs are all claims.
	const unclaimed = (label: string, base: string) => [
		base,
		...[...labels]
			.filter(([other]) => other !== label && other.includes(': ') && toolOf(other) === toolOf(label))
			.flatMap(([, { rules }]) => rules.flat().flatMap((match) => Object.values(match).flat()))
			.flatMap((glob) => (!glob.startsWith('!') && reaches(glob, folderOf(base)) ? [`!${glob}`] : []))
	];

	return Object.fromEntries(
		[...labels]
			.toSorted(([a], [b]) => a.localeCompare(b))
			.map(([label, { rules }]) => [
				label,
				rules.map((rule) => ({
					'changed-files': rule.flatMap((match) =>
						Object.entries(match).flatMap(([key, globs]) => {
							if (key === 'crates and plugins') return [{ 'any-glob-to-any-file': globs.toSorted() }];

							if (key === UNCLAIMED) return globs.map((base) => ({ 'all-globs-to-any-file': unclaimed(label, base) }));

							return [{ [key]: globs }];
						})
					)
				}))
			])
	);
};

/** The labeler config for the repository in the current folder, as JSON. */
export const repoConfig = (): string => {
	const folder = '.github/labeler';

	const files = readdirSync(folder)
		.filter((name) => name.endsWith('.yml'))
		.map((name) => ({ name, text: readFileSync(join(folder, name), 'utf8') }));

	const metadata: CargoMetadata = JSON.parse(
		execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], { encoding: 'utf8' })
	);

	return `${JSON.stringify(buildConfig(files, metadata), null, 2)}\n`;
};
