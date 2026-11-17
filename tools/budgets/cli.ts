// The command line of budgets.ts and baseline.ts, which say what they do. baseline.ts needs the
// root's dependencies, so it's only loaded for its command.
const { run } = process.argv[2] === 'baseline' ? await import('./baseline.ts') : await import('./budgets.ts');

const { lines, ok } = run(process.argv.slice(2));

for (const line of lines) console.log(line);

process.exitCode = ok ? 0 : 1;
