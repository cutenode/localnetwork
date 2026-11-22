// `node tools/crates-io/cli.ts check | new | status | setup`: names.ts says what check and new do,
// setup.ts what status and setup do. This wires them to the real commands, crates.io and the
// terminal.
import { spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline/promises';
import { Writable } from 'node:stream';
import { client } from './client.ts';
import { type Ports, type Run, check, unpublished } from './names.ts';
import { setup, status } from './setup.ts';

const run: Run = (command, args, { input, env, show = false } = {}) => {
	const result = spawnSync(command, args, {
		input,
		encoding: 'utf8',
		env: { ...process.env, ...env },
		stdio: show ? 'inherit' : 'pipe'
	});

	return { status: result.status ?? 1, stdout: result.stdout ?? '', stderr: result.stderr ?? '' };
};

// What's typed isn't shown: readline turns the terminal's echo off and writes its own into nothing.
const ask = async (prompt: string): Promise<string> => {
	process.stdout.write(prompt);

	const terminal = createInterface({
		input: process.stdin,
		output: new Writable({ write: (_chunk, _encoding, done) => done() }),
		terminal: true
	});

	const answer = await terminal.question('');

	terminal.close();
	process.stdout.write('\n');

	return answer;
};

const command = new Map([
	['check', check],
	['new', unpublished],
	['status', status],
	['setup', setup]
]).get(process.argv[2] ?? '');

const ports: Ports = {
	run,
	crates: client(fetch),
	ask,
	env: process.env,
	log: (line) => process.stdout.write(`${line}\n`)
};

if (command === undefined) {
	console.error('Usage: node tools/crates-io/cli.ts check | new | status | setup');
	process.exitCode = 1;
} else {
	try {
		process.exitCode = (await command(ports)) ? 0 : 1;
	} catch (error) {
		console.error(error instanceof Error ? error.message : String(error));
		process.exitCode = 1;
	}
}
