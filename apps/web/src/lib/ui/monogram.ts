import { hueFor } from '#lib/format.ts';
import colors from '../../../../../packages/tokens/colors.json' with { type: 'json' };
import { onColour, type Swatch } from './contrast.ts';

const palette = Object.values(colors.trackColors);

/** A name's monogram colours: one of the track colours, chosen by the name so a project keeps it everywhere. */
export const monogramSwatch = (name: string): Swatch => onColour(palette[hueFor(name) % palette.length]);
