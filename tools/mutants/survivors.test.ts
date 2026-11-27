import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { run } from './survivors.ts';

const FILE = 'crates/loclib-analysis/src/estimate.rs';

// Names as cargo-mutants gives them, with the punctuation a regular expression reads as its own.
const named = (line: number, change = 'replace > with < in estimate') => `${FILE}:${line}:37: ${change}`;

const RENDER = "replace + with * in <impl Widget for WaveOverview<'_>>::render";

const SCHEMA =
	'replace <impl JsonSchema for Alias>::schema_name -> std::borrow::Cow<\'static, str> with Cow::Owned("".to_owned())';

const GUARD = "replace match guard c == '[' || x.is_some_and(|y| y ^ z) with true in p{}$?";

let root: string;

const git = (...args: string[]) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();

const write = (path: string, text: string) => {
	mkdirSync(join(root, path, '..'), { recursive: true });
	writeFileSync(join(root, path), text);
};

beforeEach(() => {
	root = mkdtempSync(join(tmpdir(), 'mutants-'));
	git('init', '-q');
});

afterEach(() => rmSync(root, { recursive: true, force: true }));

const COMMIT = '0123456789abcdef0123456789abcdef01234567';

const RUN = 'https://github.com/cutenode/localnetwork/actions/runs/45212120059';

const list = (commit = COMMIT, url = RUN) => run(['list', join(root, 'mutants'), commit, url], root);

const cut = (count: number, url = RUN) =>
	`And ${count} more surviving mutants, in the mutants artifacts of ${url}: no run tries those again, so this issue stays open until someone closes it.`;

describe('list', () => {
	it("names every shard's missed mutants once, from both its runs, and the commit they're in", () => {
		write('mutants/mutants-0/missed.txt', `${named(35)}\n`);
		write('mutants/mutants-0/listed/mutants.out/missed.txt', `${named(12)}\n${named(35)}\n`);
		write('mutants/mutants-1/missed.txt', '');
		write('mutants/mutants-1/caught.txt', `${named(99)}\n`);

		expect(list()).toEqual({
			lines: [
				`Surviving mutants at ${COMMIT} (exclude an equivalent one in .cargo/mutants.toml's exclude_re, with the reason):`,
				'',
				`- \`${named(12)}\``,
				`- \`${named(35)}\``
			],
			ok: true
		});
	});

	it('finds one shard alone, unpacked without a folder of its own', () => {
		write('mutants/missed.txt', `${named(35)}\n`);

		expect(list().lines).toContain(`- \`${named(35)}\``);
	});

	it('cuts a list too long for an issue, saying how many more there are and where', () => {
		const names = Array.from({ length: 2000 }, (_, index) => named(1000 + index));

		write('mutants/missed.txt', names.map((name) => `${name}\n`).join(''));

		const { lines } = list();
		const listed = lines.filter((line) => line.startsWith('- '));

		// GitHub takes 65,536 characters, and the issue says more than the list.
		expect(lines.join('\n').length).toBeLessThan(51_000);
		expect(lines.join('\n').length).toBeGreaterThan(49_000);
		expect(listed).toEqual(names.slice(0, listed.length).map((name) => `- \`${name}\``));
		expect(lines.slice(-2)).toEqual(['', cut(2000 - listed.length)]);
	});

	it('says nothing when none were missed, or no shard kept a list', () => {
		expect(list()).toEqual({ lines: [], ok: true });

		write('mutants/mutants-0/missed.txt', '');

		expect(list()).toEqual({ lines: [], ok: true });
	});
});

describe('again', () => {
	const numbered = (count: number) => Array.from({ length: count }, (_, index) => `line ${index + 1}`);

	const source = (lines: string[]) => write(FILE, `${lines.join('\n')}\n`);

	/** The commit of the source as it is now: its tree, which Git compares as it does a commit's. */
	const commit = () => {
		git('add', FILE);

		return git('write-tree');
	};

	let at: string;

	beforeEach(() => {
		source(numbered(40));
		at = commit();
	});

	const post = (body: string | null, login = 'github-actions[bot]') => ({ user: { login }, body });

	/** What `list` says when these were missed. */
	const listing = (missed: string[], in_ = at) => {
		write('mutants/missed.txt', missed.map((name) => `${name}\n`).join(''));

		return list(in_).lines.join('\n');
	};

	// The same change on every line of the file, as the names cargo-mutants would list.
	const everywhere = numbered(50).map((_, index) => named(index + 1));

	/** The options for an issue whose posts are these, the comments a page each, out of `names`. */
	const options = (posts: ReturnType<typeof post>[], names = everywhere) => {
		const [issue, ...comments] = posts;

		write('issue.json', JSON.stringify(issue));
		write('comments.json', JSON.stringify(comments.map((comment) => [comment])));
		write('names.txt', names.map((name) => `${name}\n`).join(''));

		const files = ['issue.json', 'comments.json', 'names.txt'].map((name) => join(root, name));
		const { lines, ok } = run(['again', ...files], root);

		expect(ok).toBe(true);

		return lines;
	};

	const matched = (lines: string[], names: string[]) => {
		const patterns = lines.flatMap((line) => (line.startsWith('--re=') ? [new RegExp(line.slice(5))] : []));

		return names.filter((name) => patterns.some((pattern) => pattern.test(name)));
	};

	/** Which of `names` the issue's posts have cargo-mutants make. */
	const chosen = (posts: ReturnType<typeof post>[], names = everywhere) => matched(options(posts, names), names);

	it('chooses each listed mutant alone while its file is as it was', () => {
		const listed = [named(35), named(35, RENDER), named(35, SCHEMA), named(35, GUARD)];
		const lines = options([post(listing(listed))], [...everywhere, ...listed]);

		expect(lines).toHaveLength(5);
		expect(lines).toContain(`--file=${FILE}`);
		expect(matched(lines, [...listed, named(34), named(36)])).toEqual(listed);
		expect(matched(lines, [`${named(35)}s`, `x${named(35)}`, named(35).replace('estimate.rs', 'estimate_rs')])).toEqual(
			[]
		);
	});

	it('chooses what is in its place when its name is another by now', () => {
		const renamed = [named(35, 'replace > with < in guess'), named(35, 'replace > with == in guess')];
		const names = [named(34, 'replace > with < in guess'), ...renamed, `${FILE}:35:9: replace guess -> f64 with 0.0`];

		expect(chosen([post(listing([named(35)]))], names)).toEqual(renamed);
	});

	it('follows it into the file its own is called now', () => {
		const body = listing([named(35)]);
		const moved = 'crates/loclib-analysis/src/estimate/mod.rs';
		const names = everywhere.map((name) => name.replace(FILE, moved));

		rmSync(join(root, FILE));
		write(moved, `${numbered(40).toSpliced(4, 0, 'new').join('\n')}\n`);
		git('add', '.');

		expect(chosen([post(body)], names)).toEqual([names[35]]);
	});

	it('refuses names in colour, which match nothing listed', () => {
		write('issue.json', JSON.stringify(post(listing([named(35)]))));
		write('comments.json', '[]');
		write('names.txt', `${FILE}:35:37: replace \u001b[38;5;13mestimate\u001b[0m\n`);

		const files = ['issue.json', 'comments.json', 'names.txt'].map((name) => join(root, name));

		expect(() => run(['again', ...files], root)).toThrow('names in colour: list them with --colors never');
	});

	it('chooses nothing once its code is gone', () => {
		const body = listing([named(35)]);

		rmSync(join(root, FILE));
		git('add', FILE);

		expect(options([post(body)], [named(35).replace(FILE, 'crates/loclib-analysis/src/lib.rs')])).toEqual([]);
	});

	it.each([
		['lines added above it', numbered(40).toSpliced(4, 0, 'new', 'new'), 37],
		['a line added right above it', numbered(40).toSpliced(34, 0, 'new'), 36],
		['a line added right below it', numbered(40).toSpliced(35, 0, 'new'), 35],
		['lines removed above it', numbered(40).toSpliced(4, 3), 32],
		[
			'a line above it rewritten as two, and one below it removed',
			numbered(40).toSpliced(37, 1).toSpliced(9, 1, 'a', 'b'),
			36
		]
	])('follows its line past %s', (_, edited, line) => {
		const body = listing([named(35)]);

		source(edited);

		expect(chosen([post(body)])).toEqual([named(line)]);
	});

	it('chooses the change anywhere in the file once its own line has changed', () => {
		const body = listing([named(35)]);

		source(numbered(40).toSpliced(34, 1, 'rewritten'));

		expect(chosen([post(body)], [...everywhere, named(35, RENDER)])).toEqual(everywhere);
	});

	it('reads the issue and every comment, each in its own commit, whatever else they say', () => {
		const said = `Failed in https://example.com/1:\n\n- [mutants (0)](https://example.com/2)\n\n${listing([named(35)])}\n\n- wav: lines 50% (was 60%)`;

		source(['new', ...numbered(40)]);

		const later = commit();

		// The first one again, where it is by then, and two more as the website's editor saves them.
		const edited = listing([named(3), named(4)], later).replaceAll('\n', '\r\n');

		expect(options([post(said), post(listing([named(36)], later)), post(edited)])).toEqual([
			'--file=crates/loclib-analysis/src/estimate.rs',
			'--re=^crates/loclib-analysis/src/estimate\\.rs:36:37: replace > with < in estimate$',
			'--re=^crates/loclib-analysis/src/estimate\\.rs:3:37: replace > with < in estimate$',
			'--re=^crates/loclib-analysis/src/estimate\\.rs:4:37: replace > with < in estimate$'
		]);
	});

	it("leaves out what anyone but the workflows' bot posted, a post with no body, and an item without its commit", () => {
		expect(
			options([
				post(listing([named(1)]), 'someone'),
				post(listing([named(2)]), 'github-actions'),
				post(null),
				post(`- \`${named(3)}\``)
			])
		).toEqual([]);
	});
});

describe('more', () => {
	const more = (...bodies: [login: string, body: string | null][]) => {
		const [issue, ...comments] = bodies.map(([login, body]) => ({ user: { login }, body }));

		write('issue.json', JSON.stringify(issue));
		write('comments.json', JSON.stringify([comments]));

		return run(['more', join(root, 'issue.json'), join(root, 'comments.json')], root);
	};

	it('says each cut the workflows posted, once', () => {
		const bot = 'github-actions[bot]';
		const listed = `Surviving mutants at ${COMMIT} (…):\n\n- \`${named(1)}\`\n\n`;

		expect(
			more(
				[bot, `${listed}${cut(3)}`],
				[bot, `${cut(3)}\r\n${cut(1, `${RUN}/2`)}\r\n`],
				[bot, null],
				['someone', cut(9)]
			)
		).toEqual({ lines: [cut(3), cut(1, `${RUN}/2`)], ok: true });
	});

	it("reads back what the list says of one it's cut", () => {
		write('mutants/missed.txt', Array.from({ length: 2000 }, (_, index) => `${named(1000 + index)}\n`).join(''));

		const { lines } = list();

		expect(more(['github-actions[bot]', lines.join('\n')]).lines).toEqual(lines.slice(-1));
	});

	it('says nothing of a whole list', () => {
		write('mutants/missed.txt', `${named(1)}\n`);

		expect(more(['github-actions[bot]', list().lines.join('\n')])).toEqual({ lines: [], ok: true });
	});
});

it.each([
	[[]],
	[['list', 'a', 'b']],
	[['list', 'a', 'b', 'c', 'd']],
	[['again', 'a', 'b']],
	[['again', 'a', 'b', 'c', 'd']],
	[['more', 'a']],
	[['more', 'a', 'b', 'c']],
	[['other', 'a', 'b', 'c']]
])('gives the usage for %j', (args) => {
	expect(run(args)).toEqual({
		lines: [
			'usage: cli.ts list <mutants> <commit> <run> | again <issue.json> <comments.json> <names> | more <issue.json> <comments.json>'
		],
		ok: false
	});
});
