// Stand-ins for the tools the verify scripts need, for their tests.
import { chmodSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll } from 'vitest';

const root = mkdtempSync(join(tmpdir(), 'verify-tools-'));

afterAll(() => rmSync(root, { recursive: true, force: true }));

// One stand-in that prints the `.out` file beside the name it was started as, and succeeds. Each PATH
// links its tools to it: macOS scans a new executable the first time it runs, for most of a second,
// and a link to one it has scanned starts at once.
const fake = join(root, 'fake');

writeFileSync(fake, '#!/bin/sh\n/bin/cat "$0.out"\n');

chmodSync(fake, 0o755);

/** A PATH with only stand-ins on it, each printing its line. */
export const path = (tools: Record<string, string>) => {
	const bin = mkdtempSync(join(root, 'bin-'));

	for (const [name, output] of Object.entries(tools)) {
		symlinkSync(fake, join(bin, name));
		writeFileSync(join(bin, `${name}.out`), output);
	}

	return { PATH: bin };
};
