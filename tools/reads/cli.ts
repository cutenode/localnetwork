// The command line of reads.ts: what each project reads from outside its folder, among the tracked
// files and new ones not yet added, as JSON for tools/policy.sh.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { reads } from './reads.ts';

const files = execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard'], { encoding: 'utf8' })
	.split('\n')
	.filter(Boolean);

process.stdout.write(JSON.stringify({ reads: reads(files, (file) => readFileSync(file, 'utf8')) }));
