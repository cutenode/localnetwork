// A wide capture imported at the widths it's shown at, from a phone up to a 2x desktop. Each width set
// needs its own declaration, since a module pattern can only start or end with its wildcard.
declare module '*.png?enhanced&w=2560;1600;960' {
	import type { Picture } from '@sveltejs/enhanced-img';

	const value: Picture;
	export default value;
}
