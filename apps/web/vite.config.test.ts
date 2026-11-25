import { readdirSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { SHARED } from './vite.config.ts';

const LIB = join(import.meta.dirname, 'src/lib');

const modules = readdirSync(LIB, { recursive: true, encoding: 'utf8' }).filter((file) =>
	/\.(ts|js|svelte)$/.test(file)
);

// What the shared file may take: anything in src/lib but a page-only module, the Tools pages' components and edit mode.
const mayShare = (file: string) => !/\.page-only\./.test(file) && !/^components[\\/](tools|edit)[\\/]/.test(file);

it("keeps page-only modules, the Tools pages' components and edit mode out of the shared file", () => {
	expect(modules.filter((file) => SHARED.test(join(LIB, file)) !== mayShare(file))).toEqual([]);
});

it('reads Windows paths and the queries Vite adds to a module', () => {
	expect(SHARED.test('C:\\app\\src\\lib\\components\\Follow.page-only.svelte?svelte&type=style&lang.css')).toBe(false);
	expect(SHARED.test('C:\\app\\src\\lib\\components\\tools\\Card.svelte')).toBe(false);
	expect(SHARED.test('C:\\app\\src\\lib\\components\\Card.svelte')).toBe(true);
});
