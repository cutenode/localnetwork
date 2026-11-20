// The command line of budgets.ts, baseline.ts and creep.ts, which say what they do. baseline.ts
// needs the root's dependencies, so each module is only loaded for its own commands.
async function commands(command: string | undefined) {
	if (command === 'baseline') return import('./baseline.ts');

	if (command === 'creep') return import('./creep.ts');

	return import('./budgets.ts');
}

const { run } = await commands(process.argv[2]);

const { lines, ok } = run(process.argv.slice(2));

for (const line of lines) console.log(line);

process.exitCode = ok ? 0 : 1;
