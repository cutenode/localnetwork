/**
 * An uploaded mix report (or one read from another DJ's PDS) through parseMixReportText: any bytes either parse or come
 * back as an error a DJ can read, and a report that parses holds what the viewer relies on.
 */
import { parseMixReport, parseMixReportText } from '#lib/mix/schema.ts';

export const seeds = ['fuzz/seeds/mix-report', 'src/lib/mix/fixtures'];

export function fuzz(data: Buffer): void {
	const result = parseMixReportText(new Uint8Array(data));

	if (!result.ok) {
		if (!result.error) throw new Error('an unparsed report has no error to show');

		return;
	}

	const { plays, transitions } = result.report;

	if (transitions.some((t) => t.from >= plays.length || t.to >= plays.length))
		throw new Error('a transition points past the plays');
	// The site stores the parsed report as JSON (forms/project.ts), which has to parse again to the same JSON.
	const stored = JSON.stringify(result.report);
	const again = parseMixReport(JSON.parse(stored));

	if (!again.ok || JSON.stringify(again.report) !== stored) throw new Error('a stored report does not parse to itself');
}
