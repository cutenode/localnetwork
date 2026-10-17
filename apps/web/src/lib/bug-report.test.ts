import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { discussionUrl, githubIssueUrl, REPORT_LIMITS, reportBody, USERINPUT } from './bug-report';
import { validateRecord } from './server/atproto/lexicons';

describe('githubIssueUrl', () => {
	it("opens the website's bug form with the page and browser filled in", () => {
		const url = new URL(githubIssueUrl({ page: '/match', browser: 'Chrome 141, macOS' }));

		expect(url.origin + url.pathname).toBe('https://github.com/cutenode/localnetwork/issues/new');
		expect(Object.fromEntries(url.searchParams)).toEqual({
			template: 'web-bug.yml',
			steps: '1. Open /match\n2. ',
			browser: 'Chrome 141, macOS'
		});
	});

	it('carries over what someone wrote, cut short enough for GitHub to take', () => {
		const url = new URL(githubIssueUrl({}, { title: 'Blank map', body: 'x'.repeat(5_000) }));

		expect(url.searchParams.get('title')).toBe('Blank map');
		expect(url.searchParams.get('what-happened')).toHaveLength(2_000);
		expect(url.searchParams.has('steps')).toBe(false);
	});

	it('only fills fields the issue form has', () => {
		const form = readFileSync(new URL('../../../../.github/ISSUE_TEMPLATE/web-bug.yml', import.meta.url), 'utf8');
		const ids = new Set(form.matchAll(/^\s+id: (\S+)$/gm).map(([, id]) => id));
		const url = new URL(githubIssueUrl({ page: '/', browser: 'Firefox 143, Linux' }, { title: 't', body: 'b' }));
		const fields = [...url.searchParams.keys()].filter((field) => field !== 'template' && field !== 'title');

		expect(fields.filter((field) => !ids.has(field))).toEqual([]);
		expect(fields).toHaveLength(3);
	});
});

describe('reportBody', () => {
	it('adds the page and browser under what they wrote', () => {
		expect(reportBody('It froze.', { page: '/match', browser: 'Chrome 141, macOS' })).toBe(
			'It froze.\n\nPage: /match\nBrowser: Chrome 141, macOS'
		);
		expect(reportBody('It froze.', { browser: 'Chrome 141, macOS' })).toBe('It froze.\n\nBrowser: Chrome 141, macOS');
		expect(reportBody('It froze.', {})).toBe('It froze.');
	});
});

describe('REPORT_LIMITS', () => {
	it('keeps the longest report the form takes within a post, in three-byte characters too', () => {
		const space = {
			uri: 'at://did:plc:x/app.userinput.space/3mabc',
			cid: 'bafyreihyytwaxao5vpobookcw66x5w6inbc7ciiyrl6frxjweitkkngi54'
		};

		const facts = { page: `/${'漢'.repeat(499)}`, browser: 'Microsoft Edge 141, Windows' };
		const body = reportBody('漢'.repeat(REPORT_LIMITS.body), facts);
		const report = { space, title: '漢'.repeat(REPORT_LIMITS.title), body, createdAt: '2026-10-17T00:00:00.000Z' };

		expect(() => validateRecord(USERINPUT.discussion, report)).not.toThrow();
		expect(() => validateRecord(USERINPUT.discussion, { ...report, title: `${report.title}漢` })).toThrow(
			'title must not be longer than 600 characters'
		);
	});
});

describe('discussionUrl', () => {
	it("links to the post on userinput.app by its author's DID and record key", () => {
		expect(discussionUrl('at://did:plc:abc/app.userinput.discussion/3mabc')).toBe(
			'https://userinput.app/d/did:plc:abc/3mabc'
		);
	});
});
