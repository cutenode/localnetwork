import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { missing } from './tools.ts';

describe('missing', () => {
	// A PATH with only a stand-in for go on it.
	const bin = mkdtempSync(join(tmpdir(), 'verify-tools-'));
	afterAll(() => rmSync(bin, { recursive: true, force: true }));
	writeFileSync(join(bin, 'go'), '#!/bin/sh\n');
	chmodSync(join(bin, 'go'), 0o755);
	const env = { PATH: bin };

	it("names each of the script's tools the PATH lacks, with how to install it", () => {
		expect(missing('verify:repo', env)).toEqual(['lychee is missing, which verify:repo needs: brew install lychee']);
		expect(missing('verify:rust', env)).toEqual([
			'dist is missing, which verify:rust needs: cargo binstall cargo-dist@0.33.0'
		]);
	});

	it('passes a script that needs nothing, with nothing on the PATH', () => {
		expect(missing('verify', { PATH: '' })).toEqual([]);
	});
});
