// Hypeddit (hypeddit.com), the one site the panel walks itself.
//
// The walk is ../walks/hypeddit.ts, reached through ../walks/index.ts by the
// panel and the offscreen page. It reads gate pages with DOMParser, which the
// background worker does not have, and the worker reads this registry: kept
// apart, the worker's bundle never holds the walk.

import type { Site } from './index.ts';

export default {
	id: 'hypeddit',
	name: 'Hypeddit',
	hosts: ['hypeddit.com'],
	subfolder: 'Hypeddit',
	how: 'walk'
} satisfies Site;
