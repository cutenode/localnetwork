// A lock file, so two runs on one machine (from two checkouts, say) never share a fixed folder.
import { readFileSync, rmSync, writeFileSync } from 'node:fs';

/** Whether a process with this pid is running. One of another user's can't be signalled, but runs. */
function running(pid: number): boolean {
	try {
		process.kill(pid, 0);

		return true;
	} catch (error) {
		// SAFETY: process.kill throws Node's system errors, which carry a code.
		return (error as NodeJS.ErrnoException).code === 'EPERM';
	}
}

/**
 * Takes the lock at `path`, a file holding this process's pid, and returns what lets it go. A lock
 * whose process has gone, killed before it could let go, is taken over; a live one's is refused.
 */
export function lock(path: string): () => void {
	try {
		writeFileSync(path, String(process.pid), { flag: 'wx' });
	} catch (error) {
		// SAFETY: writeFileSync throws Node's system errors, which carry a code.
		if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error;
		const holder = Number(readFileSync(path, 'utf8'));

		if (holder > 0 && running(holder)) {
			throw new Error(`${path} is held by another run, process ${holder}`, { cause: error });
		}

		rmSync(path);

		return lock(path);
	}

	return () => rmSync(path, { force: true });
}
