// `node tools/verify/cli.ts <script>`, which the root's verify scripts are: run.ts and changed.ts say
// what they do.
import { changedFiles, verifyChanged } from './changed.ts';
import { verify } from './run.ts';

const script = process.argv[2] ?? '';

const { lines, ok } = script === 'verify:changed' ? verifyChanged(changedFiles(process.cwd())) : verify(script);

for (const line of lines) console.log(line);

process.exitCode = ok ? 0 : 1;
