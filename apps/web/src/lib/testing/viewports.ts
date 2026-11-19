import { MINIMAL_VIEWPORTS } from 'storybook/viewport';

/** The widths stories are drawn at: the toolbar's list (.storybook/preview.ts), and the sizes the story tests set. */
export const VIEWPORTS = {
	phone: { name: 'Phone (390)', styles: { width: '390px', height: '844px' }, type: 'mobile' },
	tablet: MINIMAL_VIEWPORTS.tablet,
	desktop: { name: 'Desktop (1280)', styles: { width: '1280px', height: '800px' }, type: 'desktop' }
} as const;

const at = (value: keyof typeof VIEWPORTS) => ({ viewport: { value, isRotated: false } });

/** A story's `globals` for each width. */
export const phone = at('phone');

export const tablet = at('tablet');

export const desktop = at('desktop');
