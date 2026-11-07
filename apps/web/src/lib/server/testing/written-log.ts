import type { JsonObject } from '@atproto/lex-json';
import { onTestFinished, vi } from 'vitest';
import { createLogger } from '../logger.ts';

export type WrittenNote = JsonObject & { level: string; message: string };

/** A line of the log as production writes it, parsed. */
export type WrittenLine = JsonObject & { level: string; message: string; notes?: WrittenNote[] };

let readers: WrittenLine[][] = [];

/**
 * A real logger at `level` and the lines it writes to stdout. For a test that reads what got logged: every reader in a
 * test sees every line.
 */
export function writtenLog(level = 'debug', lines: WrittenLine[] = []) {
	if (readers.length === 0) {
		const write = vi.spyOn(process.stdout, 'write').mockImplementation((line) => {
			for (const reader of readers) reader.push(JSON.parse(String(line)));

			return true;
		});

		onTestFinished(() => {
			write.mockRestore();
			readers = [];
		});
	}

	readers.push(lines);

	return {
		logger: createLogger({ level, pretty: false }),
		lines,
		notes: () => lines.flatMap((line) => line.notes ?? [])
	};
}
