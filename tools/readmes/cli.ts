// The command line of readmes.ts, which says what it does.
import { run } from './readmes.ts';

const { lines, ok } = run(process.argv.slice(2));

for (const line of lines) console.log(line);

process.exitCode = ok ? 0 : 1;
