// @vitest-environment jsdom
//
// Every Mermaid block in the apps' READMEs and diagram pages, through Mermaid's own parser. It needs
// a DOM for flowcharts and state diagrams, which is all jsdom is here: nothing is drawn.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import mermaid from 'mermaid';
import { describe, expect, it } from 'vitest';
import { diagramFiles, mermaidBlocks } from './readmes.ts';

const repo = resolve(import.meta.dirname, '../..');

describe('mermaid.parse', () => {
	it('refuses a block that is broken', async () => {
		await expect(mermaid.parse('flowchart TD\n  A[one --> B\n')).rejects.toThrow('Parse error');
	});

	it.each(['locallibrary', 'localtoolbox', 'web'])("reads every block in apps/%s's diagrams", async (app) => {
		const folder = resolve(repo, 'apps', app);
		const blocks = diagramFiles(folder).flatMap((file) => mermaidBlocks(readFileSync(resolve(folder, file), 'utf8')));

		expect(blocks.length).toBeGreaterThan(0);

		for (const block of blocks) await expect(mermaid.parse(block), block).resolves.toBeTruthy();
	});
});
