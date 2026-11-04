import { mkdirSync, mkdtempSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { ownedOrAbsent } from '../src/owned.ts';

const folder = () => mkdtempSync(join(tmpdir(), 'atstore-owned-'));

describe('ownedOrAbsent', () => {
	it("says whether this user's own folder or file is there", () => {
		const dir = folder();
		writeFileSync(join(dir, 'work.lock'), '1');

		expect(ownedOrAbsent(join(dir, 'missing'), 'directory')).toBe(false);
		expect(ownedOrAbsent(dir, 'directory')).toBe(true);
		expect(ownedOrAbsent(join(dir, 'work.lock'), 'file')).toBe(true);
	});

	it('refuses a symlink, even to a folder of its own', () => {
		const dir = folder();
		mkdirSync(join(dir, 'real'));
		symlinkSync(join(dir, 'real'), join(dir, 'work'));

		expect(() => ownedOrAbsent(join(dir, 'work'), 'directory')).toThrow(/work is a symlink/);
	});

	it('refuses a file where a folder belongs, and a folder where a file does', () => {
		const dir = folder();
		writeFileSync(join(dir, 'work'), '');

		expect(() => ownedOrAbsent(join(dir, 'work'), 'directory')).toThrow(/work isn't a directory/);
		expect(() => ownedOrAbsent(dir, 'file')).toThrow(/isn't a file/);
	});

	it("refuses a folder that's someone else's", () => {
		const dir = folder();
		const other = process.getuid!() + 1;

		expect(() => ownedOrAbsent(dir, 'directory', other)).toThrow(`belongs to user ${process.getuid!()}`);
	});
});
