// @vitest-environment jsdom
//
// Every Mermaid block in the apps' READMEs and diagram pages, through Mermaid's own parser. It needs
// a DOM for flowcharts and state diagrams, which is all jsdom is here: nothing is drawn.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import mermaid from 'mermaid';
import { describe, expect, it } from 'vitest';
import { diagramFiles, mermaidBlocks } from './readmes.ts';

const apps = resolve(import.meta.dirname, '../../apps');

const blocks = ['locallibrary', 'localtoolbox', 'web'].flatMap((app) =>
	diagramFiles(resolve(apps, app)).flatMap((file) =>
		mermaidBlocks(readFileSync(resolve(apps, app, file), 'utf8')).map((block, index) => ({
			name: `apps/${app}/${file} block ${index + 1}`,
			block
		}))
	)
);

describe('mermaid.parse', () => {
	it('refuses a block that is broken', async () => {
		await expect(mermaid.parse('flowchart TD\n  A[one --> B\n')).rejects.toThrow('Parse error');
	});

	it('finds every chart', () => {
		expect(blocks.length).toBeGreaterThan(200);
	});

	it.each(blocks)('reads $name', async ({ block }) => {
		await expect(mermaid.parse(block)).resolves.toBeTruthy();
	});
});
