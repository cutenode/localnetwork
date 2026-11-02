import { mkdtempSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { pick, save, screenshots } from '../src/captures.ts';
import type { Capture, Listing } from '../src/types.ts';

const capture = (file: string): Capture => ({ file, bytes: Buffer.from(file) });

const captures = ['player.png', 'library.png', 'drives.png', 'demo-track.json'].map(capture);

const listing: Listing = {
	id: 'test',
	name: 'test',
	eyebrow: 'test',
	icon: 'icon.svg',
	glow: '#ffffff',
	hero: { shots: ['player'], frame: { kind: 'panel' } },
	screenshots: ['library', 'player'],
	page: true,
	capture: async () => captures
};

describe('pick', () => {
	it('picks screenshots by name, in the order asked', () => {
		expect(pick(captures, ['library', 'player']).map(String)).toEqual(['library.png', 'player.png']);
	});

	it('names a screenshot that was not made, and what was', () => {
		expect(() => pick(captures, ['report'])).toThrow(
			'no report.png among player.png, library.png, drives.png, demo-track.json'
		);
	});
});

describe('screenshots', () => {
	it("is the listing's screenshots alone, numbered in its order", () => {
		expect(screenshots(listing, captures)).toEqual([
			{ file: 'screenshot-1-library.png', field: 'screenshots', png: Buffer.from('library.png') },
			{ file: 'screenshot-2-player.png', field: 'screenshots', png: Buffer.from('player.png') }
		]);
	});
});

describe('save', () => {
	it('leaves the folder holding the files and nothing else', () => {
		const folder = join(mkdtempSync(join(tmpdir(), 'atstore-save-')), 'captures');
		save(folder, [capture('old.png')]);
		writeFileSync(join(folder, 'stray.txt'), 'left over');

		save(folder, captures);

		expect(readdirSync(folder).toSorted()).toEqual(['demo-track.json', 'drives.png', 'library.png', 'player.png']);
		expect(readFileSync(join(folder, 'drives.png'), 'utf8')).toBe('drives.png');
	});
});
