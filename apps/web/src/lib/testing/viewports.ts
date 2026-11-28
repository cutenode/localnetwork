import { MINIMAL_VIEWPORTS } from 'storybook/viewport';

/** The widths stories are drawn at: the toolbar's list (.storybook/preview.ts), and the sizes the story tests set. */
export const VIEWPORTS = {
	phone: { name: 'Phone (390)', styles: { width: '390px', height: '844px' }, type: 'mobile' },
	tablet: MINIMAL_VIEWPORTS.tablet,
	desktop: { name: 'Desktop (1280)', styles: { width: '1280px', height: '800px' }, type: 'desktop' },
	// The narrowest that shows a 160 column terminal whole, for locallibrary's widest widgets, and tall enough for both
	// palettes of its tallest (a 160 by 45 screen): a story past the frame's bottom is cut off in its screenshot.
	wide: { name: 'Wide (1440)', styles: { width: '1440px', height: '2400px' }, type: 'desktop' },
	// A phone's width, as tall as `wide`, for the terminal stories' phone screenshots.
	tallPhone: { name: 'Phone, tall page (390)', styles: { width: '390px', height: '2400px' }, type: 'mobile' }
} as const;

const at = (value: keyof typeof VIEWPORTS) => ({ viewport: { value, isRotated: false } });

/** A story's `globals` for each width. */
export const phone = at('phone');

export const tablet = at('tablet');

export const desktop = at('desktop');

export const wide = at('wide');

export const tallPhone = at('tallPhone');
