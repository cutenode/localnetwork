// The command line of tools.ts: `node tools/verify/cli.ts <script>` fails, with a line for each tool
// the script needs that's missing.
import { missing } from './tools.ts';

const lines = missing(process.argv[2] ?? '');

for (const line of lines) console.error(line);

process.exitCode = lines.length > 0 ? 1 : 0;
