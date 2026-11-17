// The command line of coverage.ts, which says what it does.
import { run } from './coverage.ts';

const { lines, ok } = run(process.argv.slice(2));

for (const line of lines) console.log(line);

process.exitCode = ok ? 0 : 1;
