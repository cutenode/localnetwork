// A crate whose lib.rs is `#![doc = include_str!("../README.md")]` shows its README as the front
// page of its docs, where a link relative to the repository goes nowhere: rustdoc resolves it
// against the generated page, and docs.rs has no repository around it. Such a README links web
// addresses, its own headings, or Rust items (`crate::...`) only.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const crates = resolve(import.meta.dirname, '../../crates');

const readmes = readdirSync(crates)
	.filter((name) => {
		const lib = resolve(crates, name, 'src/lib.rs');

		return existsSync(lib) && readFileSync(lib, 'utf8').includes('include_str!("../README.md")');
	})
	.map((name) => ({ name, markdown: readFileSync(resolve(crates, name, 'README.md'), 'utf8') }));

const nowhere = (markdown: string) =>
	[...markdown.matchAll(/\]\(([^)\s]+)[^)]*\)|^\[[^\]]+\]:\s*(\S+)/gm)].flatMap((match) => {
		const target = match[1] ?? match[2];

		return /^(?:https?:|mailto:|#)|::/.test(target) ? [] : [target];
	});

describe('crate docs from READMEs', () => {
	it('finds the crates', () => {
		expect(readmes.length).toBeGreaterThan(10);
	});

	it('tells a link that goes nowhere from one that works', () => {
		const markdown = '[a](../../a.json) [b](https://x.dev/b) [c](#c) [d](crate::d) [e](mailto:e@x.dev)\n[f]: f.md\n';

		expect(nowhere(markdown)).toEqual(['../../a.json', 'f.md']);
	});

	it.each(readmes)('$name links nothing relative to the repository', ({ markdown }) => {
		expect(nowhere(markdown)).toEqual([]);
	});
});
