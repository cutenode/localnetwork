import { defineConfig } from '@inlang/paraglide-js';

// Read by both the Vite plugin (dev, build, tests) and `paraglide-js compile` (`pnpm check` and `pnpm install`), so
// both write the same files.
export default defineConfig({
	outdir: './src/lib/paraglide',
	// English stays at unprefixed URLs (docs/adr/0019), so the locale comes from a cookie until a second language
	// brings the url strategy for its prefix.
	strategy: ['cookie', 'baseLocale'],
	cookieName: 'ln_locale',
	cookieSameSite: 'lax',
	// Files on disk rather than Vite's virtual modules: svelte-check and the tsx scripts read them without Vite.
	outputStructure: 'message-modules',
	emitTsDeclarations: true,
	emitReadme: false,
	emitPrettierIgnore: false
});
