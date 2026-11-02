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

	it('keeps something that went wrong up for longer than something that went right', async () => {
		vi.useFakeTimers();
		const toasts = await freshToasts();
		toasts.ok('copied');
		toasts.bad('couldn’t copy');
		vi.advanceTimersByTime(2600);
		expect(toasts.items.map((t) => t.message)).toEqual(['couldn’t copy']);
		vi.advanceTimersByTime(3399);
		expect(toasts.items).toHaveLength(1);
		vi.advanceTimersByTime(1);
		expect(toasts.items).toEqual([]);
	});

	it('stops counting down while it is held, and finishes what it had left once let go', async () => {
		vi.useFakeTimers();
		const toasts = await freshToasts();
		const id = toasts.info('copied', 1000);
		vi.advanceTimersByTime(400);
		toasts.hold(id);
		// Held twice over (the pointer and the focus) is still held once.
		toasts.hold(id);
		vi.advanceTimersByTime(60_000);
		expect(toasts.items.map((t) => t.message)).toEqual(['copied']);

		toasts.release(id);
		toasts.release(id);
		// 600ms were left, but a toast let go of gets a second to be finished with.
		vi.advanceTimersByTime(999);
		expect(toasts.items).toHaveLength(1);
		vi.advanceTimersByTime(1);
		expect(toasts.items).toEqual([]);
	});

	it('gives back what it had left when that is more than a second', async () => {
		vi.useFakeTimers();
		const toasts = await freshToasts();
		const id = toasts.bad('couldn’t save');
		vi.advanceTimersByTime(1000);
		toasts.hold(id);
		vi.advanceTimersByTime(10_000);
		toasts.release(id);
		vi.advanceTimersByTime(4999);
		expect(toasts.items).toHaveLength(1);
		vi.advanceTimersByTime(1);
		expect(toasts.items).toEqual([]);
	});

	// Its close button is pressed with the pointer over it, so it is held as it goes.
	it('puts a held one away when it is dismissed', async () => {
		const toasts = await freshToasts();
		const id = toasts.bad('couldn’t save');
		toasts.hold(id);
		toasts.dismiss(id);
		expect(toasts.items).toEqual([]);
	});

	it('ignores holding or letting go of one that has gone', async () => {
		const toasts = await freshToasts();
		const id = toasts.info('gone');
		toasts.dismiss(id);
		toasts.hold(id);
		toasts.release(id);
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
