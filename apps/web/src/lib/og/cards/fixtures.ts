import { sampleReport } from '#lib/mix/sample.ts';
import type { PerformerCardProps } from './PerformerCard.svelte';

/** A made-up picture, `width`×`height`, in place of the JPEG the server makes from a stored one. */
const picture = (width: number, height: number, from: string, to: string) =>
	`data:image/svg+xml;base64,${btoa(
		`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}"><defs><linearGradient id="g" x2="1" y2="1"><stop offset="0" stop-color="${from}"/><stop offset="1" stop-color="${to}"/></linearGradient></defs><rect width="100%" height="100%" fill="url(#g)"/><circle cx="${width * 0.7}" cy="${height * 0.35}" r="${height * 0.22}" fill="#ece9df" opacity="0.5"/></svg>`
	)}`;

export const avatar = picture(360, 360, '#3987e5', '#d95926');

export const photo = picture(1200, 630, '#5a5a70', '#d95926');

export const performer: PerformerCardProps = {
	name: 'Neon Tide',
	kind: 'Solo DJ',
	city: 'Berlin',
	genres: ['techno', 'electro', 'ambient', 'dub techno'],
	avatar
};

/** A name too long for two lines at any size, to show where it's cut. */
export const longName = 'The Extremely Long Named Collective of Late Night Selectors and Friends';

export const { waveform } = sampleReport;
