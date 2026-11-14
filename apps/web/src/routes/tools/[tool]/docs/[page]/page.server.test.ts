import { describe, expect, it } from 'vitest';
import { load } from './+page.server.ts';
import type { PageServerData } from './$types';

const locals = { app: { config: { publicUrl: 'https://localnetwork.test' } } };

// SAFETY: the load reads only the params and the public URL, and returns its data synchronously.
const guide = (tool: string, page: string) => load({ params: { tool, page }, locals } as never) as PageServerData;

describe('a guide', () => {
	it('lists its sections by their heading ids when it has four or more', () => {
		expect(guide('localtoolbox', 'downloads').contents).toEqual([
			{ id: 'the-managers-list', title: "The Manager's list" },
			{ id: 'where-each-track-downloads', title: 'Where each track downloads' },
			{ id: 'working-through-a-lot-at-once', title: 'Working through a lot at once' },
			{ id: 'the-session-queue', title: 'The session queue' },
			{ id: 'download-gates', title: 'Download gates' },
			{ id: 'when-a-gate-needs-you', title: 'When a gate needs you' }
		]);
	});

	it('lists none with fewer', () => {
		expect(guide('localtoolbox', 'artists').contents).toEqual([]);
	});

	it("is a 404 when there's no such guide", () => {
		expect(() => guide('localtoolbox', 'readme')).toThrow(expect.objectContaining({ status: 404 }));
	});

	it("renders a tool's changelog from its CHANGELOG.md, without the notes for maintainers", () => {
		const { doc } = guide('locallibrary', 'changelog');

		expect(doc).toMatchObject({ title: 'Changelog', path: 'apps/locallibrary/CHANGELOG.md' });
		expect(doc.html).toMatch(/^<h2 id="unreleased">/);
		expect(doc.html).not.toContain('Keep a Changelog');
	});
});
