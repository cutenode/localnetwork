// src/lib/bug-report.ts: the settings sheet's link to GitHub's bug form.
//
// The form is read here as GitHub reads it, so a field renamed or an option
// reworded there fails this rather than leaving the link filling in nothing.
import { describe, it, expect, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { parse } from 'yaml';
import { browserLabel, bugReportUrl, extensionVersion } from '../../src/lib/bug-report.ts';
import { TOOLS } from '../../src/lib/tools.ts';
import manifest from '../../src/manifest.json';

interface FormField {
	type: string;
	id?: string;
	attributes: { options?: string[] };
}

const FORM: { body: FormField[] } = parse(
	readFileSync(resolve(import.meta.dirname, '../../../../.github/ISSUE_TEMPLATE/localtoolbox-bug.yml'), 'utf8')
);

const FIELD_IDS = FORM.body.flatMap((field) => (field.id ? [field.id] : []));

const PARTS = FORM.body.find((field) => field.id === 'tool')!.attributes.options!;

const MANIFEST_VERSION = manifest.version;

const CHROME_MAC =
	'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) ' +
	'Chrome/141.0.7390.65 Safari/537.36';

const useAgent = (userAgent: string) => vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(userAgent);

const fieldsOf = (href: string) => {
	const url = new URL(href);

	return { base: `${url.origin}${url.pathname}`, fields: Object.fromEntries(url.searchParams) };
};

describe('the version', () => {
	it('is the manifest’s, alone when the build knew no commit', () => {
		expect(extensionVersion()).toBe(MANIFEST_VERSION);
	});

	it('carries the commit in the shape the form’s placeholder asks for', () => {
		vi.stubEnv('VITE_COMMIT', 'be03543');
		expect(extensionVersion()).toBe(`${MANIFEST_VERSION} (be03543)`);
	});

	it('leaves out a commit the build could not find', () => {
		vi.stubEnv('VITE_COMMIT', '');
		expect(extensionVersion()).toBe(MANIFEST_VERSION);
	});
});

describe('the browser', () => {
	it('is the name, the major version and the OS, without the OS version Chrome freezes', () => {
		expect(browserLabel(CHROME_MAC)).toBe('Chrome 141, macOS');
	});

	it('says nothing about an agent that names no browser', () => {
		expect(browserLabel('')).toBe(undefined);
		expect(browserLabel('a-made-up-agent')).toBe(undefined);
	});

	it('drops what the agent does not say', () => {
		expect(browserLabel('Mozilla/5.0 (X11; Linux x86_64) Gecko/20100101 Firefox')).toBe('Firefox, Linux');
	});
});

describe('the link', () => {
	it('opens the localtoolbox form with the version, the browser and the open tool filled in', () => {
		useAgent(CHROME_MAC);
		vi.stubEnv('VITE_COMMIT', 'be03543');

		expect(fieldsOf(bugReportUrl('manager'))).toEqual({
			base: 'https://github.com/cutenode/localnetwork/issues/new',
			fields: {
				template: 'localtoolbox-bug.yml',
				tool: 'Manager',
				version: `${MANIFEST_VERSION} (be03543)`,
				browser: 'Chrome 141, macOS'
			}
		});
	});

	it('leaves out what it does not know rather than sending it empty', () => {
		useAgent('');

		expect(fieldsOf(bugReportUrl(null)).fields).toEqual({
			template: 'localtoolbox-bug.yml',
			version: MANIFEST_VERSION
		});
		expect(fieldsOf(bugReportUrl('soundcloud-lyrics')).fields).not.toHaveProperty('tool');
	});

	it('fills only fields the form has, and picks only parts it lists', () => {
		useAgent(CHROME_MAC);

		for (const tool of TOOLS) {
			const { template, ...fields } = fieldsOf(bugReportUrl(tool.id)).fields;
			expect(template).toBe('localtoolbox-bug.yml');

			for (const id of Object.keys(fields)) expect(FIELD_IDS, id).toContain(id);
			expect(PARTS, tool.id).toContain(fields.tool);
		}
	});
});
