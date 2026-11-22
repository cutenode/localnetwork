import { describe, expect, it } from 'vitest';
import { matrixDocs, matrixRows } from './link-matrix.ts';

// The check modules' names. They render Svelte pages, so only the browser tests load them.
const modules = Object.keys(import.meta.glob('./link-matrix/*.ts')).map((path) =>
	path.slice('./link-matrix/'.length, -'.ts'.length)
);

// A table line with one space each side of a cell, and a divider of three dashes, so a wider cell
// in one row leaves the other rows as they were.
const unpadded = (line: string) => {
	const cells = line
		.slice(1, -1)
		.split('|')
		.map((cell) => cell.trim());

	const isDivider = cells.every((cell) => /^-+$/.test(cell));

	return `|${cells.map((cell) => (isDivider ? ' --- ' : cell ? ` ${cell} ` : ' ')).join('|')}|`;
};

describe('docs/link-matrix/', () => {
	it('has a file for every check module', () => {
		expect(modules.filter((name) => !(name in matrixDocs)).map((name) => `${name}.md`)).toEqual([]);
	});

	it('has a check module for every file with a row marked on main', () => {
		const onMain = Object.entries(matrixDocs).filter(([, markdown]) =>
			matrixRows(markdown).some((row) => row.onMain === 'on main')
		);

		expect(onMain.filter(([name]) => !modules.includes(name)).map(([name]) => `${name}.ts`)).toEqual([]);
	});

	it("doesn't pad its tables", () => {
		const padded = Object.entries(matrixDocs).flatMap(([name, markdown]) =>
			markdown
				.split('\n')
				.flatMap((line, i) => (line.startsWith('|') && line !== unpadded(line) ? [`${name}.md:${i + 1}`] : []))
		);

		expect(padded).toEqual([]);
	});
});
