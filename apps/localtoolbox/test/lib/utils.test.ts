// src/lib/utils.ts: merging class lists.
import { describe, it, expect } from 'vitest';
import { cn } from '../../src/lib/utils.ts';

describe('cn', () => {
	it('takes the theme’s named font sizes for sizes, not colours', () => {
		// A small primary button: its size must not cost it the colour of its label.
		expect(cn('bg-fg text-fg-inverse', 'text-caption')).toBe('bg-fg text-fg-inverse text-caption');
		// And a later size still replaces an earlier one.
		expect(cn('text-label', 'text-caption')).toBe('text-caption');
	});
});
