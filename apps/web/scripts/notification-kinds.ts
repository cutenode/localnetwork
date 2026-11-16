/**
 * Writes the type of notifications/kinds.ts KINDS from the files in notifications/kinds/, so a producer can only name a
 * kind that's there, with the data that kind keeps. It's generated, not checked in, so that adding a kind only adds a
 * file. `pnpm install` and `pnpm check` run this.
 */
import { readdirSync, writeFileSync } from 'node:fs';

const notifications = new URL('../src/lib/server/notifications/', import.meta.url);

const kinds = readdirSync(new URL('kinds/', notifications))
	.filter((file) => file.endsWith('.ts'))
	.map((file) => file.slice(0, -'.ts'.length))
	.toSorted();

writeFileSync(
	new URL('kinds.generated.ts', notifications),
	`export type Kinds = {\n${kinds.map((name) => `\t'${name}': typeof import('./kinds/${name}.ts').default;\n`).join('')}};\n`
);
