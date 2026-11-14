// The walks: for each walked site (`how: 'walk'` in ../sites/index.ts), the
// code that fetches its gate and posts back what the page would. Imported by
// the panel and the offscreen page only, never by the background worker, which
// has no DOMParser to read a page with.
//
// Adding a walked site is its id in WalkedId (../sites/index.ts) and its walk
// here; the type below refuses one without the other.

import type { UnlockDetails, Unlocked, WalkedId, WalkedSite } from '../sites/index.ts';
import { unlock as hypeddit } from './hypeddit.ts';

const WALKS: Record<WalkedId, (pageUrl: string, details: UnlockDetails) => Promise<Unlocked>> = { hypeddit };

/** Walks one of `site`'s gates (../sites/index.ts says what it answers). */
export const walk = (site: WalkedSite, pageUrl: string, details: UnlockDetails) => WALKS[site.id](pageUrl, details);
