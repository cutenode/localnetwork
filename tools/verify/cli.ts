// `node tools/verify/cli.ts <script>`, which the root's verify scripts are: run.ts says what it does.
import { verify } from './run.ts';

const { lines, ok } = verify(process.argv[2] ?? '');

for (const line of lines) console.log(line);

process.exitCode = ok ? 0 : 1;
