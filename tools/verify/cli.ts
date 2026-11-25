// `node tools/verify/cli.ts <script> [--package <crate>]`, which the root's verify scripts are:
// run.ts, changed.ts and crate.ts say what they do.
import { parseArgs } from 'node:util';
import { changedFiles, verifyChanged } from './changed.ts';
import { verifyCrate } from './crate.ts';
import { type Verdict, verify } from './run.ts';

const result = (): Verdict => {
	let parsed;

	try {
		parsed = parseArgs({ options: { package: { type: 'string', short: 'p' } }, allowPositionals: true });
	} catch (error) {
		return { lines: [error instanceof Error ? error.message : String(error)], ok: false };
	}

	const {
		positionals: [script = ''],
		values
	} = parsed;

	if (values.package !== undefined)
		return script === 'verify:rust'
			? verifyCrate(values.package)
			: { lines: [`--package goes with verify:rust, not ${script}`], ok: false };

	if (script === 'verify:changed') return verifyChanged(changedFiles(process.cwd()));

	return verify(script);
};

const { lines, ok } = result();

for (const line of lines) console.log(line);

process.exitCode = ok ? 0 : 1;
