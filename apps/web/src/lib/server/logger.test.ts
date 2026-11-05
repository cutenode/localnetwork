import { describe, expect, it } from 'vitest';
import { createLogger } from './logger';

describe('createLogger', () => {
	it('logs at the level asked for, pretty or as JSON, under the name given or localnetwork', () => {
		const json = createLogger({ level: 'warn', pretty: false });
		const pretty = createLogger({ level: 'debug', pretty: true, name: 'syncer' });

		expect([json.level, json.bindings()]).toEqual(['warn', { name: 'localnetwork' }]);
		expect([pretty.level, pretty.bindings()]).toEqual(['debug', { name: 'syncer' }]);
	});

	it('writes each level by name, so failures do not show as info in the Railway logs', () => {
		const lines: Array<{ level: string; msg: string }> = [];

		const logger = createLogger({
			level: 'info',
			pretty: false,
			stream: { write: (line) => void lines.push(JSON.parse(line)) }
		});

		logger.info('listening');
		logger.warn('space sync failed');
		logger.error('bootstrap failed');

		expect(lines.map(({ level, msg }) => [level, msg])).toEqual([
			['info', 'listening'],
			['warn', 'space sync failed'],
			['error', 'bootstrap failed']
		]);
	});
});
