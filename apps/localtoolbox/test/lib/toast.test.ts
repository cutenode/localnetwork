// src/lib/toast.svelte.ts — the messages that come up at the bottom of the
// popup and go away on their own. What they say is asserted where they are
// said, in each panel's tests; what is here is that they go, and when.
import { describe, it, expect, vi } from 'vitest';

async function freshToasts() {
	vi.resetModules();
	const { toasts } = await import('../../src/lib/toast.svelte.ts');

	return toasts;
}

describe('a toast', () => {
	it('stays for its time, and not a moment longer', async () => {
		vi.useFakeTimers();
		const toasts = await freshToasts();
		toasts.show('copied', 'ok', 100);
		expect(toasts.items.map((t) => t.message)).toEqual(['copied']);
		vi.advanceTimersByTime(99);
		expect(toasts.items).toHaveLength(1);
		vi.advanceTimersByTime(1);
		expect(toasts.items).toEqual([]);
	});

	it('goes by itself, one at a time, without taking the others with it', async () => {
		vi.useFakeTimers();
		const toasts = await freshToasts();
		toasts.info('first', 100);
		toasts.info('second', 300);
		vi.advanceTimersByTime(100);
		expect(toasts.items.map((t) => t.message)).toEqual(['second']);
		// The default is long enough to read and short enough to be gone.
		toasts.info('third');
		vi.advanceTimersByTime(2599);
		expect(toasts.items.map((t) => t.message)).toEqual(['third']);
		vi.advanceTimersByTime(1);
		expect(toasts.items).toEqual([]);
	});

	it('says whether something worked in the tone it deserves', async () => {
		const toasts = await freshToasts();
		toasts.said(true, 'copied', 'couldn’t copy');
		toasts.said(false, 'copied', 'couldn’t copy');
		expect(toasts.items.map(({ message, tone }) => [message, tone])).toEqual([
			['copied', 'ok'],
			['couldn’t copy', 'bad']
		]);
	});

	it('can be taken away early', async () => {
		const toasts = await freshToasts();
		const id = toasts.bad('nope');
		toasts.info('kept');
		toasts.dismiss(id);
		expect(toasts.items.map((t) => t.message)).toEqual(['kept']);
	});
});
