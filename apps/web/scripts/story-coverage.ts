/**
 * Fails when a component in src/lib has no story and no exemption in .storybook/story-exemptions.json,
 * or an exemption is stale. `pnpm lint` runs this, so a new component gets a story in the same change.
 */
import { join } from 'node:path';
import { storyCoverage } from './lib/story-coverage.ts';

const problems = storyCoverage(join(import.meta.dirname, '..'));

for (const problem of problems) console.error(problem);

process.exitCode = problems.length > 0 ? 1 : 0;
