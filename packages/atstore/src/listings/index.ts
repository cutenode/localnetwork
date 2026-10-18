// The tools with an AT Store listing. A new one is one file here, listed below.
import { locallibrary } from './locallibrary.ts';
import { localtoolbox } from './localtoolbox.ts';
import { web } from './web.ts';
import type { Listing } from '../types.ts';

export const listings: Listing[] = [web, locallibrary, localtoolbox];
