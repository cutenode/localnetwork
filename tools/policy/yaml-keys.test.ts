import { readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { expect, it } from 'vitest';
import { parse } from 'yaml';

const github = resolve(import.meta.dirname, '../../.github');

const files = readdirSync(github).filter((name) => name.endsWith('.yml'));

// conftest keeps the last of two equal keys without a word, so when a merge adds the same label to
// labeler.yml from both sides, the policies only see one side's globs. yaml's parse throws on it.
it.each(files)('.github/%s has no repeated keys', (name) => {
	expect(() => parse(readFileSync(resolve(github, name), 'utf8'))).not.toThrow();
});
