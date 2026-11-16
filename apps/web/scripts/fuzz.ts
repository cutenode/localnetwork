/**
 * Fuzzes the byte parsers with Jazzer.js (libFuzzer for Node), one target in fuzz/ after another, from apps/web:
 *
 *   pnpm fuzz                                   # every target, 30 seconds each
 *   pnpm fuzz --seconds 300 range               # one target, for five minutes
 *   pnpm fuzz range --input fuzz/artifacts/range/crash-…   # replay a crash
 *
 * Jazzer's loader instruments JavaScript only, so esbuild bundles each target to fuzz/build/ first, leaving the
 * packages out. The instrumented code is the bundle, zod (the mix report schema) and image-size, which is what tells
 * the fuzzer an input reached somewhere new. Each target mutates its `seeds` folders, keeps what reaches new code in
 * fuzz/corpus/<target>/ and writes the input of a crash to fuzz/artifacts/<target>/ (all but the seeds ignored by
 * git). Any crash fails the run, after every target has had its turn.
 */
import { spawnSync } from 'node:child_process';
import { mkdirSync, readdirSync } from 'node:fs';
import { parseArgs } from 'node:util';
import { build } from 'esbuild';

const { values, positionals } = parseArgs({
	options: { seconds: { type: 'string', default: '30' }, input: { type: 'string' } },
	allowPositionals: true
});

const targets = positionals.length
	? positionals
	: readdirSync('fuzz')
			.filter((file) => file.endsWith('.fuzz.ts'))
			.map((file) => file.slice(0, -'.fuzz.ts'.length));

await build({
	entryPoints: targets.map((target) => `fuzz/${target}.fuzz.ts`),
	outdir: 'fuzz/build',
	outExtension: { '.js': '.mjs' },
	bundle: true,
	platform: 'node',
	format: 'esm',
	packages: 'external',
	logLevel: 'warning'
});

// What Jazzer instruments: the bundle, zod (which runs the mix report schema) and image-size. Setting excludes replaces
// its default, node_modules, which would leave those two out.
const instrument = ['fuzz/build/', '/zod/', '/image-size/'].flatMap((path) => ['--includes', path]);

let failed = 0;

for (const target of targets) {
	const bundle = `fuzz/build/${target}.fuzz.mjs`;
	// SAFETY: every target exports its seed folders as `seeds`.
	const { seeds } = (await import(`../${bundle}`)) as { seeds: string[] };
	const corpus = `fuzz/corpus/${target}`;
	const artifacts = `fuzz/artifacts/${target}/`;
	mkdirSync(corpus, { recursive: true });
	mkdirSync(artifacts, { recursive: true });
	// libFuzzer adds what it finds to the first folder and only reads the others. One file runs once instead.
	const inputs = values.input ? [values.input] : [corpus, ...seeds];
	const limits = values.input ? [] : [`-max_total_time=${values.seconds}`];
	console.log(`fuzzing ${target}`);

	const { status } = spawnSync(
		'jazzer',
		[
			bundle,
			...inputs,
			'--sync',
			...instrument,
			'--excludes',
			'/@jazzer.js/',
			'--',
			...limits,
			`-artifact_prefix=${artifacts}`
		],
		{ stdio: 'inherit' }
	);

	if (status !== 0) failed++;
}

if (failed) {
	console.error(`${failed} of ${targets.length} targets crashed: the inputs are in fuzz/artifacts/`);
	process.exit(1);
}
