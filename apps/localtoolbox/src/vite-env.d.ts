// What Svelte and Vite let an import be (a component, a stylesheet), for the type check.
/// <reference types="svelte" />
/// <reference types="vite/client" />

interface ImportMetaEnv {
	/** The short commit the build was made from (vite.config.ts): '' without git, and absent under Vitest. */
	readonly VITE_COMMIT?: string;
}
