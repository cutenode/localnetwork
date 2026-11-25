// From the repository root: `node tools/regen/cli.ts driver <path>` is the merge driver
// (merge.regen.driver, which tools/lefthook-rc.sh sets), and `post-merge`, `post-commit` and
// `pre-push` are lefthook's jobs of those names. regen.ts says what each does.
import { readFileSync } from 'node:fs';
import { committed, driver, regenerate, unpushable } from './regen.ts';

const [step, path] = process.argv.slice(2);

const root = process.cwd();

if (step === 'driver' && path) driver(root, path);
else if (step === 'post-merge') process.exitCode = regenerate(root, console.error) ? 0 : 1;
else if (step === 'post-commit') process.exitCode = committed(root, console.error) ? 0 : 1;
else if (step === 'pre-push') {
	// The refs being pushed, which Git gives the hook on stdin. Run by hand there are none.
	const { pending, uncommitted } = unpushable(root, process.stdin.isTTY ? '' : readFileSync(0, 'utf8'));

	if (pending.length > 0)
		console.error(`Regenerate these first, with \`node tools/regen/cli.ts post-merge\`: ${pending.join(', ')}`);

	if (uncommitted.length > 0)
		console.error(`Commit the files regenerated after the merge first: ${uncommitted.join(', ')}`);

	if (pending.length > 0 || uncommitted.length > 0) process.exitCode = 1;
} else {
	console.error('usage: node tools/regen/cli.ts driver <path> | post-merge | post-commit | pre-push');
	process.exitCode = 2;
}
