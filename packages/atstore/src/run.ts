// Running the tools' own builds and servers, from the repository root.
import { spawn, type ChildProcess, type StdioOptions } from 'node:child_process';
import { once } from 'node:events';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

/** The repository root, which every command runs from unless it says otherwise. */
export const ROOT = join(import.meta.dirname, '../../..');

/** Where a command runs, what it adds to the environment, and what interrupts it. */
type Options = { cwd?: string; env?: Record<string, string>; signal?: AbortSignal };

/** Ends a child's process group, and so everything it started, unless it's over already. */
function end(child: ChildProcess) {
	if (child.exitCode !== null || child.signalCode !== null) return;
	// SAFETY: a detached child that is still running has a pid, which is its group's.
	process.kill(-child.pid!, 'SIGTERM');
}

/**
 * Starts a command in a process group of its own, which `signal` ends: a terminal's Ctrl-C reaches
 * only atstore, which passes it on to every group, so each one ends whatever its command started.
 */
function spawnGroup(command: string, args: string[], { cwd = ROOT, env = {}, signal }: Options, stdio: StdioOptions) {
	signal?.throwIfAborted();
	const child = spawn(command, args, { cwd, env: { ...process.env, ...env }, stdio, detached: true });
	const interrupt = () => end(child);
	signal?.addEventListener('abort', interrupt, { once: true });
	child.once('exit', () => signal?.removeEventListener('abort', interrupt));

	return child;
}

/** Runs a command to the end with its output showing, and fails if it does. */
export async function run(command: string, args: string[], options: Options = {}): Promise<void> {
	const child = spawnGroup(command, args, options, ['ignore', 'inherit', 'inherit']);
	const [code, signal] = await once(child, 'exit');

	if (code !== 0) throw new Error(`${command} ${args.join(' ')} exited with ${code ?? signal}`);
}

/** A command left running, such as a server. */
export interface Running {
	stop(): Promise<void>;
}

/**
 * Starts a command that keeps running and resolves once a line of its output matches `ready`.
 * `stop` ends it with everything it started, such as the servers it runs.
 */
export async function start(
	command: string,
	args: string[],
	{ ready, timeout = 240_000, ...options }: Options & { ready: RegExp; timeout?: number }
): Promise<Running> {
	const child = spawnGroup(command, args, options, ['ignore', 'pipe', 'pipe']);
	const exited = once(child, 'exit');
	const tail: string[] = [];

	const stop = async () => {
		end(child);
		await exited;
	};

	const seen = new Promise<void>((resolve, reject) => {
		const timer = setTimeout(
			() => reject(new Error(`${command} wasn't ready in ${timeout / 1000}s:\n${tail.join('\n')}`)),
			timeout
		);

		for (const stream of [child.stdout, child.stderr]) {
			// SAFETY: both are piped, above.
			createInterface({ input: stream! }).on('line', (line) => {
				tail.push(line);

				if (tail.length > 20) tail.shift();

				if (!ready.test(line)) return;

				clearTimeout(timer);
				resolve();
			});
		}

		child.once('exit', (code, signal) => {
			clearTimeout(timer);
			reject(new Error(`${command} exited with ${code ?? signal} before it was ready:\n${tail.join('\n')}`));
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
