/**
 * Where each of `count` hot cues sits in a track `total` seconds long: spread evenly, and each moved
 * onto the start of the nearest phrase, where a DJ would set one, when that's close enough (a fifth
 * of the spacing) to keep the cues clear of each other.
 */
export function cuePoints(count: number, total: number, phrases: { start: number }[]): number[] {
	const spacing = total / count;

	return Array.from({ length: count }, (_, i) => {
		const even = spacing * (i + 0.5);

		const nearest = phrases.reduce(
			(best, { start }) => (Math.abs(start - even) < Math.abs(best - even) ? start : best),
			Infinity
		);

		return Math.abs(nearest - even) <= spacing / 5 ? nearest : even;
	});
}
