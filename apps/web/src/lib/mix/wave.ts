/** A made-up waveform of `n` columns, 0 to 255, for samples and decoration. `seed` shifts its shape. */
export const madeUpWave = (n: number, seed: number, base: number, swing: number) =>
	Array.from({ length: n }, (_, i) =>
		Math.round(base + swing * Math.abs(Math.sin(i / 7 + seed) * Math.cos(i / 31 + seed)))
	);

/** A made-up mix waveform in its three bands, `n` columns long. */
export const madeUpBands = (n: number) => ({
	low: madeUpWave(n, 1, 40, 200),
	mid: madeUpWave(n, 2, 30, 160),
	high: madeUpWave(n, 3, 20, 120)
});
