// The mutants the weekly mutants job missed, as the weekly issue lists them, and back again: the next
// run mutates them once more, so the issue stays open until a test kills them. A week's run otherwise
// mutates only that week's changes, where an older survivor isn't.
//
//   node tools/mutants/cli.ts list <mutants> <commit> <run>  the list, out of every shard's missed.txt
//       anywhere under <mutants>, in <commit>, cut to what an issue holds with how many more there are
//       in the artifacts of <run>, its address; nothing when they're empty
//   node tools/mutants/cli.ts again <issue.json> <comments.json> <names>  the options, one a line, that
//       have cargo mutants make only the ones listed in the open issue or the pages of its comments (as
//       GitHub's REST API has them), out of <names>, what `cargo mutants --list` says of the checkout
//   node tools/mutants/cli.ts more <issue.json> <comments.json>  what they say of the ones that were
//       cut, which nothing makes again: each later run fails on them, which keeps the issue open
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import type { Outcome } from '../coverage/coverage.ts';

interface Post {
	user: { login: string };
	body: string | null;
}

// Anyone can comment on the issue, but only the workflows' token posts as this: no user's name has brackets.
const BOT = 'github-actions[bot]';

const HEADING = /^Surviving mutants at ([0-9a-f]{40}) /;

const ITEM = /^- `(\S+\.rs:\d+:\d+: .+)`$/;

const CUT = /^And \d+ more surviving mutants, in the mutants artifacts of /;

// How long a list gets, in characters: GitHub takes 65,536 in an issue or a comment, which say more than it.
const LIMIT = 50_000;

const NAME = /^(\S+):(\d+):(\d+): (.+)$/;

const HUNK = /^@@ -(\d+)(?:,(\d+))? \+\d+(?:,(\d+))? @@/;

/** The Markdown naming the mutants in the missed.txt files under `mutants`, which are of `commit` and the run at `url`. */
export function list(mutants: string, commit: string, url: string): string[] {
	if (!existsSync(mutants)) return [];

	const missed = readdirSync(mutants, { recursive: true, encoding: 'utf8' })
		.filter((path) => basename(path) === 'missed.txt')
		.flatMap((path) => readFileSync(join(mutants, path), 'utf8').split('\n'))
		.filter(Boolean);

	if (missed.length === 0) return [];

	const items = [...new Set(missed)].sort().map((name) => `- \`${name}\``);
	let length = 0;
	const fit = items.filter((item) => (length += item.length + 1) <= LIMIT);
	const cut = items.length - fit.length;

	return [
		`Surviving mutants at ${commit} (exclude an equivalent one in .cargo/mutants.toml's exclude_re, with the reason):`,
		'',
		...fit,
		...(cut === 0
			? []
			: [
					'',
					`And ${cut} more surviving mutants, in the mutants artifacts of ${url}: no run tries those again, so this issue stays open until someone closes it.`
				])
	];
}

/** Every line of the workflows' posts, post by post. */
const said = (posts: Post[]) =>
	posts.flatMap(({ user, body }) => (user.login === BOT ? [(body ?? '').split(/\r?\n/)] : []));

/** What the workflows' posts say of the survivors cut from their lists. */
export function more(posts: Post[]): string[] {
	return [...new Set(said(posts).flatMap((lines) => lines.filter((line) => CUT.test(line))))];
}

const parsed = (name: string) => {
	const [, file, line, column, change] = NAME.exec(name) ?? [];

	return { name, file, line: Number(line), column, change };
};

const diff = (root: string, ...args: string[]) =>
	execFileSync('git', ['diff', '--no-ext-diff', '-M', ...args], { cwd: root, encoding: 'utf8', maxBuffer: 2 ** 28 });

/** What each file of `commit` that has moved since is called in the checkout at `root`. */
function moves(root: string, commit: string): Map<string, string> {
	const fields = diff(root, '-z', '--name-status', '--diff-filter=R', commit).split('\0');

	return new Map(
		Array.from({ length: Math.floor(fields.length / 3) }, (_, index) => [fields[3 * index + 1], fields[3 * index + 2]])
	);
}

/** Where `line` of `commit`'s `file`, called `now` in the checkout at `root`, is there, or nowhere when it's one of those changed since. */
function lineNow(root: string, commit: string, file: string, now: string, line: number): number | undefined {
	const hunks = diff(root, '-U0', commit, '--', file, now)
		.split('\n')
		.flatMap((text) => {
			const [matched, start, removed = '1', added = '1'] = HUNK.exec(text) ?? [];

			return matched === undefined ? [] : [{ start: Number(start), removed: Number(removed), added: Number(added) }];
		});

	// A hunk that removes nothing adds its lines after `start`.
	const above = hunks.filter(({ start, removed }) => line >= start + Math.max(removed, 1));
	const changed = hunks.some(({ start, removed }) => line >= start && line < start + removed);

	return changed ? undefined : above.reduce((sum, { removed, added }) => sum + added - removed, line);
}

const escaped = (text: string) => text.replace(/[\\.+*?()|[\]{}^$]/g, '\\$&');

/**
 * A `--re` for each of `names` that the workflows' posts list: the mutant at its line, followed through
 * the changes since the list's commit, or, when its line has changed, the same change anywhere in its
 * file. One at its line whose name is another by now (its function is renamed, say) is whichever are
 * there. A `--file` for each of their files too, as cargo-mutants 27.1.0 tries every deleted field
 * whatever `--re` says.
 */
export function again(posts: Post[], names: string[], root: string): string[] {
	if (names.some((name) => name.includes('\u001b'))) throw new Error('names in colour: list them with --colors never');

	const all = names.map(parsed);
	const moved = new Map<string, Map<string, string>>();

	const placed = (commit: string, survivor: string) => {
		const { file: was, line, column, change } = parsed(survivor);

		if (!moved.has(commit)) moved.set(commit, moves(root, commit));

		const file = moved.get(commit)?.get(was) ?? was;
		const now = lineNow(root, commit, was, file, line);
		const here = all.filter((mutant) => mutant.file === file);

		if (now === undefined) return here.filter((mutant) => mutant.change === change);

		const there = here.filter((mutant) => mutant.line === now && mutant.column === column);
		const same = there.filter((mutant) => mutant.change === change);

		return same.length > 0 ? same : there;
	};

	const survivors = said(posts).flatMap((lines) => {
		let commit: string | undefined;

		return lines.flatMap((text) => {
			commit = HEADING.exec(text)?.[1] ?? commit;

			const survivor = ITEM.exec(text)?.[1];

			return survivor === undefined || commit === undefined ? [] : placed(commit, survivor);
		});
	});

	return [...new Set(survivors.flatMap(({ name, file }) => [`--file=${file}`, `--re=^${escaped(name)}$`]))];
}

// SAFETY: what GitHub's REST API answered.
const readJson = <T>(path: string) => JSON.parse(readFileSync(path, 'utf8')) as T;

/** Runs a command and gives the lines to print and whether it passed; `again` from the checkout at `root`. */
export function run([command, ...args]: string[], root = '.'): Outcome {
	const [first = '', second = '', third = ''] = args;
	const posts = () => [readJson<Post>(first), ...readJson<Post[][]>(second).flat()];

	if (command === 'list' && args.length === 3) return { lines: list(first, second, third), ok: true };

	if (command === 'more' && args.length === 2) return { lines: more(posts()), ok: true };

	if (command === 'again' && args.length === 3) {
		return { lines: again(posts(), readFileSync(third, 'utf8').split('\n'), root), ok: true };
	}

	return {
		lines: [
			'usage: cli.ts list <mutants> <commit> <run> | again <issue.json> <comments.json> <names> | more <issue.json> <comments.json>'
		],
		ok: false
	};
}
