import { z } from 'zod';
import registry from './services.json' with { type: 'json' };

const envName = z.string().regex(/^[A-Z][A-Z0-9_]*$/);

const service = z
	.object({
		title: z.string().min(1).describe('The service as people know it, for error messages: "SoundCloud".'),
		tokenUrl: z
			.url({ protocol: /^https$/ })
			.describe("The service's token endpoint. The exchange sends nothing anywhere else."),
		style: z
			.enum(['form', 'query'])
			.describe(
				'How the service takes a token request. form: a POST with a form body, client_id and client_secret in it, for codes and refresh tokens (RFC 6749). query: a GET with client_id, client_secret, redirect_uri and code in the query, for codes only (Mixcloud).'
			),
		clientIdEnv: envName.describe("The environment variable holding Local Network's client id at the service."),
		clientSecretEnv: envName.describe("The environment variable holding that client's secret.")
	})
	.strict();

export const servicesSchema = z
	.object({
		$schema: z.string().optional(),
		services: z
			.record(z.string().regex(/^[a-z0-9-]+$/), service)
			.describe("By the id a plugin's account names at the end of its token_exchange address.")
	})
	.strict()
	.describe(
		"The services Local Network's token exchange (/api/token-exchange/<id>) signs locallibrary plugins in to, adding the client secret only this server holds."
	);

export type Service = z.infer<typeof service>;

/** The checked-in registry: a bad edit fails its test, and every exchange until it is put right. */
export const services: Record<string, Service> = servicesSchema.parse(registry).services;
