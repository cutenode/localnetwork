import pino from 'pino';
import { describe, expect, it } from 'vitest';
import { HappyViewError } from './client';
import { unlessOffline } from './offline';

describe('unlessOffline', () => {
	const lines: string[] = [];

	const log = pino({}, { write: (line: string) => void lines.push(line) });

	it('passes the answer through', async () => {
		expect(await unlessOffline(log, async () => 12)).toBe(12);
	});

	it('answers null and logs when HappyView is down', async () => {
		const down = new HappyViewError(503, 'HappyView /xrpc: fetch failed');

		expect(await unlessOffline(log, () => Promise.reject(down))).toBeNull();
		expect(lines.join('')).toContain('"msg":"directory offline"');
	});

	it('still throws errors that are not the directory being down', async () => {
		await expect(unlessOffline(log, () => Promise.reject(new Error('bug')))).rejects.toThrow('bug');
	});
});
