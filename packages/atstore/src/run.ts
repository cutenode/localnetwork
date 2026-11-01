// Running the tools' own builds and servers, from the repository root.
import { spawn, type SpawnOptions } from 'node:child_process';
import { once } from 'node:events';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

/** The repository root, which every command runs from unless it says otherwise. */
export const ROOT = join(import.meta.dirname, '../../..');

type Options = { cwd?: string; env?: Record<string, string> };

const spawnOptions = ({ cwd = ROOT, env = {} }: Options): SpawnOptions => ({ cwd, env: { ...process.env, ...env } });

/** Runs a command to the end with its output showing, and fails if it does. */
export async function run(command: string, args: string[], options: Options = {}): Promise<void> {
	const child = spawn(command, args, { ...spawnOptions(options), stdio: 'inherit' });
	const [code] = await once(child, 'exit');

	if (code !== 0) throw new Error(`${command} ${args.join(' ')} exited with ${code}`);
}

/** A command left running, such as a server. */
export interface Running {
	stop(): Promise<void>;
}

/**
 * Starts a command that keeps running and resolves once a line of its output matches `ready`. It
 * runs in a process group of its own, which `stop` ends, so the servers it starts end with it.
 */
export async function start(
	command: string,
	args: string[],
	{ ready, timeout = 240_000, ...options }: Options & { ready: RegExp; timeout?: number }
): Promise<Running> {
	const child = spawn(command, args, { ...spawnOptions(options), stdio: ['ignore', 'pipe', 'pipe'], detached: true });
	const exited = once(child, 'exit');
	const tail: string[] = [];

	const stop = async () => {
		if (child.exitCode !== null || child.signalCode !== null) return;
		// SAFETY: a detached child that is still running has a pid, which is its group's.
		process.kill(-child.pid!, 'SIGTERM');
		await exited;
	};

	const seen = new Promise<void>((resolve, reject) => {
		const timer = setTimeout(
			() => reject(new Error(`${command} wasn't ready in ${timeout / 1000}s:\n${tail.join('\n')}`)),
			timeout
		);

		for (const stream of [child.stdout, child.stderr]) {
			createInterface({ input: stream }).on('line', (line) => {
				tail.push(line);

				if (tail.length > 20) tail.shift();

				if (!ready.test(line)) return;

				clearTimeout(timer);
				resolve();
			});
		}

		child.once('exit', (code) => {
			clearTimeout(timer);
			reject(new Error(`${command} exited with ${code} before it was ready:\n${tail.join('\n')}`));
		});
	});

	try {
		await seen;
	} catch (error) {
		await stop();
		throw error;
	}

	return { stop };
}
