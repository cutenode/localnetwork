// Lets pending promise chains settle. Uses `setImmediate`, which the fake
// timers in vitest.config.ts leave real, so it works whether or not a test
// has frozen `setTimeout`. A few rounds cover chains of several awaits.
export const flush = async (rounds = 5) => {
	for (let i = 0; i < rounds; i++) {
		await new Promise((resolve) => setImmediate(resolve));
	}
};

// The real timer and the real clock, taken before any test can replace them.
// `vi.useFakeTimers()` freezes `setTimeout`, and `vi.setSystemTime` — which
// every test gets from test/setup.js — freezes `Date`, so neither can be used
// to wait for, or to measure, something happening outside this worker.
const realSetTimeout = globalThis.setTimeout;

const sleep = (ms: number) => new Promise((resolve) => realSetTimeout(resolve, ms));

const now = () => Number(process.hrtime.bigint() / 1000000n);

/**
 * Waits until `done()` is true, for up to `ms` of real time.
 *
 * `flush` waits a fixed number of turns, which is plenty for a chain of
 * promises already in this worker and nowhere near enough for anything that
 * has to leave it. A tool panel's chunk is a real dynamic `import()`: the
 * first one in a worker goes out to Vite for the file and comes back about
 * three seconds later, which is tens of thousands of turns. A test that
 * waited a fixed number of turns for one passed only because an earlier test
 * in the same file had already warmed that chunk — and failed, once, whenever
 * the shuffled order made it the test that paid for it.
 *
 * The budget is a backstop rather than a timeout: a condition that never
 * comes true fails on the assertion after this, which can say what was
 * expected, rather than here, which cannot. It is well inside the suite's own
 * testTimeout so that it is always the assertion that reports.
 */
export const until = async (done: () => boolean, { ms = 20000 }: { ms?: number } = {}) => {
	// Most conditions are a promise or two away; those never reach the clock.
	for (let i = 0; i < 20; i++) {
		if (done()) return true;
		await new Promise((resolve) => setImmediate(resolve));
	}

	const deadline = now() + ms;

	while (now() < deadline) {
		if (done()) return true;
		await sleep(1);
	}

	return done();
};
