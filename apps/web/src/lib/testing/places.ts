import { vi } from 'vitest';
import type { Place } from '#lib/places.ts';

/**
 * Stands in for `/api/places`: answers each query in `answers` and finds nothing for any other. Returns the URLs
 * asked for, in order. Undo it with `vi.unstubAllGlobals()`.
 */
export function servePlaces(answers: Record<string, Place[]>) {
	const asked: string[] = [];

	vi.stubGlobal('fetch', async (url: string) => {
		asked.push(url);

		return Response.json({ places: answers[new URL(url, location.href).searchParams.get('q') ?? ''] ?? [] });
	});

	return asked;
}
