import { tools } from '#lib/tools/index.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = () => ({
	tools: tools.map(({ id, name, kind, tagline }) => ({ id, name, kind, tagline }))
});
