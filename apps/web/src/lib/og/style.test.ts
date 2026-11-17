import { expect, it } from 'vitest';
import { nameSize } from './style';

it('sizes a name as large as fits, and as small as allowed past the longest', () => {
	const sizes: [chars: number, px: number][] = [
		[10, 96],
		[20, 72]
	];

	expect(nameSize('Neon Tide', sizes)).toBe(96);
	expect(nameSize('Saffron Social Club', sizes)).toBe(72);
	expect(nameSize('An act whose name runs past every size', sizes)).toBe(72);
});
