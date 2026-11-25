// `node tools/crates-io/cli.ts package`: what crates.io would get from each crate setup and
// loclib-release.yml publish, without building it. It catches a missing file, metadata crates.io
// turns down, or a dependency it can't have. Pull requests that change a crate's manifest run it
// (ci.yml's locallibrary_crates paths), and so does `pnpm verify:rust`.
//
// It names the crates rather than passing `--workspace`. `cargo publish --workspace` skips the
// `publish = false` members, but `cargo package --workspace` packages them too
// (https://github.com/rust-lang/cargo/blob/930a674da900a73c1dfbe16be62c42e75bdfdfc0/src/ops/cargo_package/mod.rs#L379-L383),
// and locallibrary's path dependency on locallibrary-core has no version, which packaging refuses.
// `--allow-dirty` because verify:rust runs it on uncommitted work too.
import { type Ports, publishable } from './names.ts';

export const packageCrates = async ({ run }: Ports): Promise<boolean> => {
	const crates = publishable(run).flatMap((crate) => ['-p', crate]);

	return run('cargo', ['package', '--no-verify', '--locked', '--allow-dirty', ...crates], { show: true }).status === 0;
};
