import { describe, expect, it } from 'vitest';
import { featuresOf, followAnchors, hasBullets, split, withFeaturesOf } from './features.ts';

const readme = (...features: string[]) =>
	`# Web\n\nIntro.\n\n## Features\n\n${features.join('\n\n')}\n\n## Setup\n\nRun it.\n`;

const tools =
	'### Tools\n\n- `/tools` lists [the hosts](src/lib/hosts.json), [bug reports](#bug-reports) and [this](#tools).';

const bugs = [
	'### Bug reports',
	'',
	'- "Report a bug" opens [GitHub](https://github.com) from [setup](#setup) and [locallibrary](../locallibrary).',
	'- Posting asks for one more OAuth scope.'
].join('\n');

const flags = '### Flags\n\nFlags are in [flags](../../flags/).\n\nEach has an owner.';

const dashboard = [
	'### Dashboard',
	'',
	'- Signing in lands on `/dashboard`. Account (`/dashboard/account`) lists sessions. The first page, Overview, sums up. Staff work in the staff console.'
].join('\n');

describe('split', () => {
	it("writes each bullet to its feature's file, with its links fixed for that folder", () => {
		const { files, unknown } = split(readme(tools, bugs, flags));

		expect(unknown).toEqual([]);
		expect(Object.fromEntries(files)).toEqual({
			'apps/web/docs/features/tools/tools.md':
				'# Tools\n\n`/tools` lists [the hosts](../../../src/lib/hosts.json), [bug reports](../bug-reports/) and [this](./).\n',
			'apps/web/docs/features/bug-reports/report-a-bug.md':
				'# Report a bug\n\n"Report a bug" opens [GitHub](https://github.com) from [setup](../../../README.md#setup) and [locallibrary](../../../../locallibrary).\n',
			'apps/web/docs/features/bug-reports/posting-scope.md':
				'# The posting scope\n\nPosting asks for one more OAuth scope.\n',
			'apps/web/docs/features/flags/flags.md':
				'# Flags\n\nFlags are in [flags](../../../../../flags).\n\nEach has an owner.\n'
		});
	});

	it("splits a bullet's sentences into the files they went to", () => {
		expect(Object.fromEntries(split(readme(dashboard)).files)).toEqual({
			'apps/web/docs/features/dashboard/dashboard.md':
				'# The dashboard\n\nSigning in lands on `/dashboard`.\n\nStaff work in the staff console.\n',
			'apps/web/docs/features/dashboard/account.md': '# Account\n\nAccount (`/dashboard/account`) lists sessions.\n',
			'apps/web/docs/features/dashboard/overview.md': '# Overview\n\nThe first page, Overview, sums up.\n'
		});
	});

	it('names the headings and bullets it has no file for', () => {
		const { files, unknown } = split(readme('### Venues\n\n- A venue page.', '### Tools\n\n- A new tool.'));

		expect(unknown).toEqual(['### Venues', '- A new tool.']);
		expect(files.size).toBe(0);
	});

	it("keeps the sentences of a split whose start an older bullet doesn't have with the piece before", () => {
		const older = '### Dashboard\n\n- Signing in lands on `/dashboard`, and more. The first page, Overview, sums up.';

		expect(Object.fromEntries(split(readme(older)).files)).toEqual({
			'apps/web/docs/features/dashboard/dashboard.md':
				'# The dashboard\n\nSigning in lands on `/dashboard`, and more.\n',
			'apps/web/docs/features/dashboard/overview.md': '# Overview\n\nThe first page, Overview, sums up.\n'
		});
	});

	it('finds no bullets in a README without a Features section', () => {
		expect(split('# Web\n').files.size).toBe(0);
	});
});

describe('the Features section', () => {
	it('runs to the next heading of its level, or the end', () => {
		expect(featuresOf(readme(tools))).toBe(`## Features\n\n${tools}\n\n`);
		expect(featuresOf(`# Web\n\n## Features\n\n${tools}\n`)).toBe(`## Features\n\n${tools}\n`);
		expect(featuresOf('# Web\n')).toBe('');
	});

	it('has bullets until it is split', () => {
		expect(hasBullets(readme(tools))).toBe(true);
		expect(hasBullets(readme('- [Tools](docs/features/tools/)'))).toBe(false);
		expect(hasBullets('# Web\n\n### Notes\n')).toBe(false);
	});

	it("takes another README's section, and leaves a README without one as it is", () => {
		const links = readme('- [Tools](docs/features/tools/)');

		expect(withFeaturesOf(readme(tools).replace('Run it.', 'Run it now.'), links)).toBe(
			links.replace('Run it.', 'Run it now.')
		);
		expect(withFeaturesOf('# Web\n', links)).toBe('# Web\n');
	});
});

describe('followAnchors', () => {
	it('points a link to a moved heading of the web README at its folder', () => {
		const text = '[a](../apps/web/README.md#dashboard), [b](../apps/web/README.md#setup), [c](../README.md#dashboard)';

		expect(followAnchors('docs/links.md', text)).toBe(
			'[a](../apps/web/docs/features/dashboard/), [b](../apps/web/README.md#setup), [c](../README.md#dashboard)'
		);
	});
});
