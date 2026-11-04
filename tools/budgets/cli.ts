// The command line of budgets.ts, which says what it does.
import { run } from './budgets.ts';

const { lines, ok } = run(process.argv.slice(2));
for (const line of lines) console.log(line);
process.exitCode = ok ? 0 : 1;
