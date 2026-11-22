import { describe, expect, it } from 'vitest';
import { metadata, world } from './fakes.ts';
import { packageCrates } from './package.ts';

const METADATA = 'cargo metadata --no-deps --format-version 1 --locked';

const PACKAGE = 'cargo package --no-verify --locked --allow-dirty -p loclib-a -p loclib-b';

const commands = { [METADATA]: { stdout: metadata(['loclib-b', null], ['locallibrary', []], ['loclib-a', null]) } };

describe('packageCrates', () => {
	it("packages only the crates that can be published, showing cargo's output", async () => {
		const { ports, ran } = world({}, commands);

		expect(await packageCrates(ports)).toBe(true);
		expect(ran.at(-1)).toEqual({ line: PACKAGE, show: true });
	});

	it('fails when cargo does', async () => {
		const { ports } = world({}, { ...commands, [PACKAGE]: { status: 101 } });

		expect(await packageCrates(ports)).toBe(false);
	});
});
