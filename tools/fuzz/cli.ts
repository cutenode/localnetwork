// The command line of targets.ts and report.ts, which say what they do.
const { run } = await(process.argv[2] === 'report' ? import('./report.ts') : import('./targets.ts'));

const { lines, ok } = run(process.argv.slice(2));

for (const line of lines) console.log(line);

process.exitCode = ok ? 0 : 1;
