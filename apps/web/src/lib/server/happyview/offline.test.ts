import { describe, expect, it } from 'vitest';
import { silentLogger } from '../logger.ts';
import { writtenLog } from '../testing/written-log.ts';
import { HappyViewError } from './client';
import { unlessOffline } from './offline';

describe('unlessOffline', () => {
	it('passes the answer through', async () => {
		expect(await unlessOffline(silentLogger, async () => 12)).toBe(12);
	});

	it('answers null and logs when HappyView is down', async () => {
		const down = new HappyViewError(503, 'HappyView /xrpc: fetch failed');

		const { logger, lines } = writtenLog();

		expect(await unlessOffline(logger, () => Promise.reject(down))).toBeNull();
		expect(lines.map((line) => line.message)).toEqual(['directory offline']);
	});

	it('still throws errors that are not the directory being down', async () => {
		await expect(unlessOffline(silentLogger, () => Promise.reject(new Error('bug')))).rejects.toThrow('bug');
	});
});
