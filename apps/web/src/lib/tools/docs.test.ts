import { describe, expect, it } from 'vitest';
import { loadDocs } from './docs';
import { findTool, tools } from './index';

describe('loadDocs', () => {
	it('puts the README first, then the guides in the order it links them, then the rest by title', () => {
		const docs = loadDocs('apps/localthing', {
			'../../../../localthing/docs/untitled.md': 'No heading.\n',
			'../../../../localthing/docs/zebra.md': '# Zebra\n\nZ.\n',
			'../../../../localthing/docs/apple.md': '# Apple\n\nA.\n',
			'../../../../localthing/docs/mango.md': '# Mango\n\nM.\n',
			'../../../../localthing/README.md': '# localthing\n\nSee [Zebra](docs/zebra.md) and [Mango](./docs/mango.md#x).\n'
		});

		expect(docs.map((doc) => doc.slug)).toEqual(['readme', 'zebra', 'mango', 'apple', 'untitled']);
		expect(docs[0]).toMatchObject({ title: 'Overview', path: 'apps/localthing/README.md' });
		expect(docs[1]).toEqual({
			slug: 'zebra',
			path: 'apps/localthing/docs/zebra.md',
			title: 'Zebra',
			markdown: '\nZ.\n'
		});
		expect(docs[4]).toMatchObject({ title: 'docs/untitled.md', markdown: 'No heading.\n' });
	});

	it('refuses files that are neither', () => {
		expect(() => loadDocs('apps/x', { '../x/src/notes.md': '' })).toThrow("isn't a README");
	});
});

describe('the registry', () => {
	it("bundles each tool's README and docs, but not the generated RPC contract", () => {
		const locallibrary = findTool('locallibrary')!;
		const slugs = locallibrary.docs.map((doc) => doc.slug);
		expect(slugs[0]).toBe('readme');
		expect(slugs).toEqual(expect.arrayContaining(['install', 'commands', 'account', 'sidecar']));
		expect(locallibrary.docs.find((doc) => doc.slug === 'install')).toMatchObject({
			title: 'Installing locallibrary',
			path: 'apps/locallibrary/docs/install.md'
		});
		expect(locallibrary.docs.some((doc) => doc.path.includes('/rpc/'))).toBe(false);
		expect(findTool('localtoolbox')?.docs.map((doc) => doc.slug)).toEqual(['readme']);
		expect(findTool('nope')).toBeUndefined();
	});

	it('gives every tool a unique id and a guide', () => {
		expect(new Set(tools.map((tool) => tool.id)).size).toBe(tools.length);

		for (const tool of tools) expect(tool.docs.length).toBeGreaterThan(0);
	});
});
