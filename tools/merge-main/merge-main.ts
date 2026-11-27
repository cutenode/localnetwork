// Brings a branch from before the hegel prop_* names, the `.page-only` modules and the split web
// README up to main, run while merging main (`pnpm merge:main`, after `git merge origin/main` stops
// on a conflict or with `--no-commit`). HEAD is the branch and MERGE_HEAD is main. In order:
//
// 1. A conflict where main's side differs from the merge base only by those renames
//    (conflicts.ts) takes this branch's side, which step 3 renames: its edit, or its deletion.
// 2. When apps/web/README.md conflicts because this branch edited the old Features bullets, those
//    edits are merged into main's docs/features/ files (features.ts), and the README takes main's
//    Features section, with the rest of it merged as usual.
// 3. The page-only modules are moved and every path naming one follows (page-only.ts), links to the
//    README's old headings point at their folders, and the hegel tests not yet named prop_* are
//    renamed (prop-rename.ts).
//
// It stages what it changed, except files that still conflict, which it lists. It never commits.
// Running it again changes nothing.
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, posix, resolve } from 'node:path';
import { renameOnly } from './conflicts.ts';
import { featuresOf, followAnchors, hasBullets, README, split, withFeaturesOf } from './features.ts';
import { moves, pathRewriter, TEXT as PATH_TEXT } from './page-only.ts';
import { HEGEL_TESTS, parseScan, propRename, TEXT as WORD_TEXT } from './prop-rename.ts';

const AST_GREP = resolve(import.meta.dirname, '../../node_modules/.bin/ast-grep');

/** Does the merge's mechanical part in the worktree at `root`; false when something needs a person first. */
export const mergeMain = (root: string, log: (line: string) => void) => {
	const git = (args: string[], input?: string) =>
		execFileSync('git', args, { cwd: root, input, encoding: 'utf8', maxBuffer: 1 << 28 });

	const read = (path: string) => (existsSync(join(root, path)) ? readFileSync(join(root, path), 'utf8') : '');

	const write = (path: string, text: string) => {
		mkdirSync(dirname(join(root, path)), { recursive: true });
		writeFileSync(join(root, path), text);
	};

	const show = (id?: string) => (id ? git(['cat-file', 'blob', id]) : '');
	let said = false;

	const say = (line: string) => {
		said = true;
		log(line);
	};

	// Each unmerged path's blobs by stage: 1 the merge base, 2 this branch, 3 main.
	const unmerged = () => {
		const stages = new Map<string, Record<string, string>>();

		for (const entry of git(['ls-files', '-u', '-z']).split('\0').filter(Boolean)) {
			const tab = entry.indexOf('\t');
			const [, id, stage] = entry.slice(0, tab).split(' ');
			const path = entry.slice(tab + 1);

			stages.set(path, { ...stages.get(path), [stage]: id });
		}

		return stages;
	};

	// A three-way merge of texts, as Git would leave a conflicting file.
	const merge = (ours: string, base: string, theirs: string) => {
		const ids = [ours, base, theirs].map((text) => git(['hash-object', '-w', '--stdin'], text).trim());

		const { stdout, status } = spawnSync(
			'git',
			['merge-file', '-p', '-q', '--object-id', '-L', 'HEAD', '-L', 'base', '-L', 'MERGE_HEAD', ...ids],
			{ cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] }
		);

		return { text: stdout, clean: status === 0, ids };
	};

	const staged = new Set<string>();
	let ok = true;

	// 1. Conflicts that are only main's renames.
	const resolved = unmerged()
		.entries()
		.filter(([, stages]) => stages[1] && stages[3] && renameOnly(show(stages[1]), show(stages[3])))
		.toArray();

	for (const [path, stages] of resolved) {
		// This branch's side is its edit, or its deletion when it removed or moved the file.
		if (stages[2]) git(['checkout', '--ours', '--', path]);
		else rmSync(join(root, path), { force: true });

		staged.add(path);
	}

	if (resolved.length > 0)
		say(`took this branch's side where main only renamed: ${resolved.map(([path]) => path).join(', ')}`);

	// 2. The README's Features bullets, while the README still conflicts on them.
	const readme = unmerged().get(README);
	const [base, ours, theirs] = [1, 2, 3].map((stage) => show(readme?.[stage]));
	const tracked = [...new Set(git(['ls-files', '-z']).split('\0').filter(Boolean))];
	const renames = moves(tracked);
	const moved = (path: string) => renames.find(([from]) => from === path)?.[1] ?? path;
	const rewrite = pathRewriter(tracked.map(moved));

	if (hasBullets(base) && !hasBullets(theirs) && featuresOf(read(README)) !== featuresOf(theirs)) {
		const before = split(base);
		const after = split(ours);
		const unknown = [...new Set([...before.unknown, ...after.unknown])];

		if (unknown.length > 0) {
			ok = false;
			say(
				`${README}: these aren't in tools/merge-main/sections.ts, so the README is left to you:\n${unknown.join('\n')}`
			);
		} else {
			const edited = [...new Set([...before.files.keys(), ...after.files.keys()])].filter(
				(path) => before.files.get(path) !== after.files.get(path)
			);

			for (const path of edited) {
				const result = merge(
					rewrite(path, after.files.get(path) ?? ''),
					rewrite(path, before.files.get(path) ?? ''),
					read(path)
				);

				if (result.clean) {
					// A bullet the branch removed takes its file with it.
					if (result.text) write(path, result.text);
					else rmSync(join(root, path));

					staged.add(path);
					continue;
				}

				// Unmerged, with each side as a stage, so Git holds the commit until it's resolved.
				write(path, result.text);
				git(['update-index', '--force-remove', '--', path]);
				git(
					['update-index', '--index-info'],
					result.ids.map((id, i) => `100644 ${id} ${[2, 1, 3][i]}\t${path}\n`).join('')
				);
			}

			const result = merge(withFeaturesOf(ours, theirs), withFeaturesOf(base, theirs), theirs);

			write(README, result.text);

			if (result.clean) staged.add(README);
			say(
				`moved this branch's edits to the Features bullets into ${edited.join(', ')}, and took main's Features section in ${README}`
			);
		}
	}

	if (staged.size > 0) git(['add', '--', ...staged]);

	// 3. The renames, across the tree.
	for (const [from, to] of renames) git(['mv', from, to]);

	if (renames.length > 0)
		say(`moved the page-only modules: ${renames.map(([from, to]) => `${from} -> ${to}`).join(', ')}`);

	// Not this tool's own files, which name what it renames.
	const before = new Map(
		tracked
			.values()
			.map(moved)
			.filter((path) => PATH_TEXT.test(path) || WORD_TEXT.test(path))
			.filter((path) => !path.startsWith('.claude/') && !path.startsWith('tools/merge-main/'))
			.filter((path) => existsSync(join(root, path)))
			.map((path) => [path, read(path)])
	);

	const texts = new Map(
		[...before].map(([path, text]) => [
			path,
			path.endsWith('.md') ? followAnchors(path, rewrite(path, text)) : rewrite(path, text)
		])
	);

	// Only the tracked files' tests: an untracked file isn't part of the merge.
	const files = new Set(tracked);

	const scan = execFileSync(AST_GREP, ['scan', '--inline-rules', HEGEL_TESTS, '--json=stream'], {
		cwd: root,
		encoding: 'utf8',
		maxBuffer: 1 << 28,
		stdio: ['ignore', 'pipe', 'ignore']
	});

	const tests = parseScan(scan).filter(({ file }) => files.has(file));

	const crates = new Set(
		tracked
			.values()
			.filter((path) => posix.basename(path) === 'Cargo.toml')
			.map((path) => posix.dirname(path))
	);

	const props = propRename(tests, new Map([...texts].filter(([path]) => WORD_TEXT.test(path))), crates);

	for (const [path, text] of props.changed) texts.set(path, text);

	const changed = [...texts].filter(([path, text]) => text !== before.get(path));
	const conflicting = unmerged();
	const toStage = changed.flatMap(([path]) => (conflicting.has(path) ? [] : [path]));

	for (const [path, text] of changed) write(path, text);

	if (toStage.length > 0) git(['add', '--', ...toStage]);

	if (props.collisions.length > 0) {
		ok = false;
		say(
			`these have a hegel test's name in the test's own file, so rename them by hand first:\n${props.collisions.join('\n')}`
		);
	}

	for (const note of props.notes) say(note);

	if (props.leftover.length > 0)
		say(`old test names left outside their crates, for a look:\n${props.leftover.join('\n')}`);

	if (changed.length > 0) say(`renamed in ${changed.length} files: ${changed.map(([path]) => path).join(', ')}`);

	if (conflicting.size > 0) say(`still unmerged, for you to resolve: ${[...conflicting.keys()].join(', ')}`);

	if (!said) log('nothing to do');

	return ok;
};
