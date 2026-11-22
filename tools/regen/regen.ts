// The files a test writes in full, regenerated after a merge instead of conflicting. Each one's
// folder has a .gitattributes line `<file> merge=regen regen=<name>`, and tools/regen/<name>.sh is
// the command that writes it, so adding one is two lines in its own folder and a file here.
//
// - Git runs `driver` for such a file when both sides changed it. It keeps this branch's copy, so
//   the merge goes through, and notes the file.
// - The post-merge hook runs `regenerate`: each noted file's command, once, then stages what the
//   commands changed among the files they own and names them. The merge commit has this branch's
//   copies, so they still need committing.
// - Git skips post-merge when the merge stops on a conflict elsewhere, so the post-commit hook runs
//   `committed`, which regenerates whatever is still noted once the merge is committed.
// - The pre-push hook runs `unpushable`, which refuses while a file is still noted, or while a pushed
//   commit has the merge's copy of a file it regenerated.
//
// A rebase, cherry-pick or `git stash pop` runs the driver but no hook that regenerates, so the
// noted files wait for pre-push, and `node tools/regen/cli.ts post-merge` regenerates them by hand.
//
// A command that fails is named with its files, which keep this branch's copy, so CI's drift tests
// fail on them as they would have.
import { execFileSync, spawnSync } from 'node:child_process';
import { appendFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

const git = (cwd: string, ...args: string[]) => execFileSync('git', args, { cwd, encoding: 'utf8' });

// For the commands whose exit code is the answer.
const exitCode = (cwd: string, ...args: string[]) => spawnSync('git', args, { cwd }).status;

// A path in this worktree's own git folder. Git gives it relative in the main worktree and absolute
// in a linked one, so it's asked for absolute.
const gitPath = (cwd: string, path: string) =>
	git(cwd, 'rev-parse', '--path-format=absolute', '--git-path', path).trim();

// A file under the worktree's git folder, so two worktrees' merges keep apart.
const note = (cwd: string, name: string) => gitPath(cwd, `regen/${name}`);

const lines = (path: string) => (existsSync(path) ? readFileSync(path, 'utf8').split('\n').filter(Boolean) : []);

// Each path's `merge` and `regen` attributes, from `git check-attr -z`'s path, attribute, value triples.
const attributes = (cwd: string, paths: string[]) => {
	const found = new Map<string, { merge?: string; regen?: string }>();

	if (paths.length === 0) return found;

	const out = execFileSync('git', ['check-attr', '-z', '--stdin', 'merge', 'regen'], {
		cwd,
		input: paths.join('\0'),
		encoding: 'utf8'
	}).split('\0');

	for (let i = 0; i + 2 < out.length; i += 3) {
		const [path, attribute, value] = [out[i], out[i + 1], out[i + 2]];
		const set = attribute === 'merge' ? { merge: value } : { regen: value };

		if (value !== 'unspecified' && value !== 'unset') found.set(path, { ...found.get(path), ...set });
	}

	return found;
};

const command = (name: string) => `tools/regen/${name}.sh`;

/** For the merge driver: keeps this branch's copy of `path` (Git's %A, left as it is) and notes it. */
export const driver = (cwd: string, path: string) => {
	const pending = note(cwd, 'pending');

	mkdirSync(dirname(pending), { recursive: true });
	appendFileSync(pending, `${path}\n`);
};

/** For the post-merge hook: regenerates the noted files and stages them. Returns whether every command passed. */
export const regenerate = (cwd: string, log: (line: string) => void): boolean => {
	const pending = note(cwd, 'pending');
	const noted = [...new Set(lines(pending))];

	rmSync(pending, { force: true });

	const found = attributes(cwd, noted);
	const names = new Set([...found.values()].flatMap(({ regen }) => (regen ? [regen] : [])));
	let passed = true;

	for (const name of names) {
		const { status } = spawnSync('bash', [command(name)], { cwd, stdio: 'inherit' });
		const own = noted.filter((path) => found.get(path)?.regen === name);

		if (status !== 0) {
			log(`${command(name)} failed, so ${own.join(', ')} keep this branch's copy: run it again, or CI's check fails`);
			passed = false;
			continue;
		}

		// What it changed among the files it writes, new and deleted ones included. A rename or copy's
		// entry is followed by its source path, as a field of its own.
		const entries = git(cwd, 'status', '--porcelain', '-z', '--untracked-files=all').split('\0');
		const changed: string[] = [];

		for (let i = 0; i < entries.length; i++) {
			if (!entries[i]) continue;

			changed.push(entries[i].slice(3));

			if (/[RC]/.test(entries[i].slice(0, 2))) changed.push(entries[++i]);
		}

		const written = [...attributes(cwd, changed)]
			.flatMap(([path, { regen }]) => (regen === name ? [path] : []))
			.toSorted();

		if (written.length === 0) continue;

		// Not `git add`, which fails on a path that's gone from both the index and the worktree, as a
		// rename's source is.
		git(cwd, 'update-index', '--add', '--remove', '--', ...written);

		const head = git(cwd, 'rev-parse', 'HEAD').trim();

		appendFileSync(note(cwd, 'staged'), written.map((path) => `${head} ${path}\n`).join(''));
		log(`Regenerated with ${command(name)} and staged, to commit: ${written.join(', ')}`);
	}

	return passed;
};

/**
 * For the post-commit hook: regenerates what's still noted, as after a merge that stopped on a
 * conflict and was then committed. Mid-rebase or mid-cherry-pick it leaves the note for pre-push,
 * since staging there would get in the next commit's way.
 */
export const committed = (cwd: string, log: (line: string) => void): boolean => {
	const replaying = ['rebase-merge', 'rebase-apply', 'sequencer'].some((path) => existsSync(gitPath(cwd, path)));

	return replaying || regenerate(cwd, log);
};

/**
 * For the pre-push hook, given its stdin (`<local ref> <local sha> <remote ref> <remote sha>` lines):
 * the files a merge noted that aren't regenerated yet, and the regenerated files that a pushed
 * commit still has the merge's copy of.
 */
export const unpushable = (cwd: string, pushed: string) => {
	const staged = note(cwd, 'staged');

	const regenerated = lines(staged).map((line) => {
		const space = line.indexOf(' ');

		return { commit: line.slice(0, space), path: line.slice(space + 1) };
	});

	const tips = pushed
		.split('\n')
		.map((line) => line.split(' ')[1])
		.filter((sha) => sha && !/^0+$/.test(sha));

	// 0 when `tip` has `commit` and still has its copy of `path`, 1 when it has changed it since.
	const since = (tip: string, { commit, path }: { commit: string; path: string }) =>
		exitCode(cwd, 'merge-base', '--is-ancestor', commit, tip) === 0
			? exitCode(cwd, 'diff', '--quiet', commit, tip, '--', path)
			: undefined;

	const uncommitted = regenerated
		.filter((entry) => tips.some((tip) => since(tip, entry) === 0))
		.map(({ path }) => path);

	// What HEAD has committed since needs no more checking.
	const left = regenerated.filter((entry) => since('HEAD', entry) !== 1);

	if (left.length === 0) rmSync(staged, { force: true });
	else writeFileSync(staged, left.map(({ commit, path }) => `${commit} ${path}\n`).join(''));

	return { pending: [...new Set(lines(note(cwd, 'pending')))], uncommitted: [...new Set(uncommitted)] };
};

/**
 * What's wrong with the generated files' setup: a file with `merge=regen` but no command, a
 * `regen` attribute without `merge=regen`, or a command no file names.
 */
export const problems = (cwd: string): string[] => {
	const found = attributes(cwd, git(cwd, 'ls-files').split('\n').filter(Boolean));

	const commands = readdirSync(join(cwd, 'tools/regen'))
		.filter((file) => file.endsWith('.sh'))
		.map((file) => file.slice(0, -'.sh'.length));

	const named = new Set([...found.values()].map(({ regen }) => regen));

	return [
		...[...found].flatMap(([path, { merge, regen }]) => {
			if (merge !== 'regen' && !regen) return [];

			if (merge !== 'regen') return [`${path}: regen=${regen} needs merge=regen too`];

			return regen && commands.includes(regen)
				? []
				: [`${path}: merge=regen needs regen=<name> and tools/regen/<name>.sh`];
		}),
		...commands.filter((name) => !named.has(name)).map((name) => `${command(name)}: no file has regen=${name}`)
	];
};
