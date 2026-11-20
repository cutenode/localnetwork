/**
 * The Lua scripts in lua/, by path from the project root (`/lua/lib/directory.lua`), bundled in when Vite builds or
 * serves the app. Vite resolves `#lua-files` here (vite.config.ts). Scripts run with tsx, which has no
 * `import.meta.glob`, so Node resolves it to scripts/lib/lua-files.ts, which reads the same files from disk.
 */
export const LUA_FILES = import.meta.glob<string>('/lua/**/*.lua', { query: '?raw', import: 'default', eager: true });
