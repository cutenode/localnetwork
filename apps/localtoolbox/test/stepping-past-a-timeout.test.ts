// A test that times out keeps running. These helpers are what stop it reaching
// into the test after it (test/helpers/panel.ts).
import { describe, it, expect, vi } from 'vitest';
import { mountPanel, panelChrome, stepFrames } from './helpers/panel.ts';

describe('a test that has outlived its timeout', () => {
	it('stops stepping the shared clock', async () => {
		vi.useFakeTimers();
		const stopped = new AbortController();
		const steps = stepFrames(4, stopped.signal);
		const before = Date.now();
		stopped.abort();

		await expect(steps).rejects.toThrow('aborted');
		expect(Date.now() - before).toBeLessThanOrEqual(60);
	});

	it('keeps stepping while the test is running', async () => {
		vi.useFakeTimers();
		const before = Date.now();
		await stepFrames(4, new AbortController().signal);
		expect(Date.now() - before).toBe(240);
	});

	it('stops a mount that would land in the next test', async () => {
		panelChrome();
		const mounting = mountPanel('manager');
		expect.setState({ currentTestName: 'the next test' });

		await expect(mounting).rejects.toThrow('this mount outlived its test');
		expect(document.querySelector('h1')).toBe(null);
	});
});
