import type { Logger } from '#lib/server/logger.ts';
import { HappyViewError } from './client';

/**
 * Runs a directory query, or returns null when HappyView is down, so a page can render its
 * static parts and say the directory is offline instead of failing whole. Other errors still throw.
 */
export async function unlessOffline<T>(log: Pick<Logger, 'warn'>, query: () => Promise<T>): Promise<T | null> {
	try {
		return await query();
	} catch (error) {
		if (!(error instanceof HappyViewError)) throw error;

		log.warn({ err: error }, 'directory offline');

		return null;
	}
}
