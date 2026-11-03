import { existsSync } from 'node:fs';

/**
 * Loads `.env.local` then `.env` from the working directory, without overriding real environment
 * variables. Only in development: production takes its environment as given, and a test run
 * (Vitest sets NODE_ENV to `test`) mustn't open the database or accounts a developer's files name.
 */
export function loadEnvFiles(): void {
	if (process.env.NODE_ENV === 'production' || process.env.NODE_ENV === 'test') return;

	for (const file of ['.env.local', '.env']) {
		if (existsSync(file)) process.loadEnvFile(file);
	}
}
