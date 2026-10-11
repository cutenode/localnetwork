# Installing locallibrary

## Install

Each [GitHub Release](https://github.com/cutenode/localnetwork/releases) has `locallibrary` for
macOS (Apple silicon and Intel), Linux (x86_64) and Windows (x86_64), and installers for it. The
Linux and Windows builds are experimental and untested on hardware (see [Platforms](#platforms)).
There hasn't been a release yet (see [Releasing](../CONTRIBUTING.md#releasing)); until then, build
from source.

```bash
# macOS and Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/cutenode/localnetwork/releases/latest/download/locallibrary-cli-installer.sh | sh

# Homebrew
brew install cutenode/tap/locallibrary
```

```powershell
# Windows
powershell -ExecutionPolicy Bypass -c "irm https://github.com/cutenode/localnetwork/releases/latest/download/locallibrary-cli-installer.ps1 | iex"
```

The shell and PowerShell installers put `locallibrary` in `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin`
on Windows). The Linux build needs glibc 2.35 or newer (Ubuntu 22.04, Debian 12) and ALSA's library
(`libasound2`, which desktops already have) for the player's audio.

From source, with Rust 1.91 or newer, in a clone of the repository:

```bash
cargo install --path apps/locallibrary --locked
```

This installs `locallibrary`. On Linux, the player's audio output needs the ALSA headers
(`sudo apt install libasound2-dev pkg-config` on Debian/Ubuntu).

Cargo features choose what's built. All four are on unless `--no-default-features` turns them
off:

- `account`: the Atmosphere account and publishing to it (`account`, `profile publish`, `sets
  publish`, `settings publish`, `settings import --from-handle` and `--from-account`, the player's
  sign-in, and the sidecar's `account.*` and `*.publish.*` methods). Half the dependencies are for
  this alone (an HTTP client, an async runtime, TLS, DNS, the system keychain): about 220 of the 530
  crates the command builds. Without it the profile and its document are still made, just not
  published.
- `tui`: the player (`locallibrary tui`, and `locallibrary` on its own in a terminal).
- `playback`: the player's audio output, and with it ALSA on Linux. Without it the player
  browses, edits and shows waveforms, but has no output to play through.
- `peers`: `store serve` and `store fetch`, copying snapshots between computers over the network
  ([iroh](https://iroh.computer)'s QUIC connections, about 80 more crates).

```bash
# A headless sidecar for a desktop app: no player, no account (about 160 crates)
cargo install --path apps/locallibrary --locked --no-default-features
# The same, with the account
cargo install --path apps/locallibrary --locked --no-default-features --features account
# The player without audio output: no ALSA headers needed on Linux
cargo install --path apps/locallibrary --locked --no-default-features --features tui,account
```

The library (`crates/locallibrary` at the repository root) has the `account` and `peers` features too, on by default.

### Shell completion and man pages
Tab completes commands and flags, and also the drives mounted now, their playlist paths, My Tags and
history lists, the libraries, sets and sessions in the store (a library's name wherever one is
taken), player models and the steps `refresh --skip` leaves out: the shell asks `locallibrary`
itself as you type. Load the script from your
shell's startup file (rather than saving it once), so it always matches the `locallibrary` you have:

```bash
echo 'source <(locallibrary completions bash)' >> ~/.bashrc
echo 'source <(locallibrary completions zsh)' >> ~/.zshrc
locallibrary completions fish > ~/.config/fish/completions/locallibrary.fish
echo 'eval (locallibrary completions elvish | slurp)' >> ~/.elvish/rc.elv
```

In PowerShell, add `locallibrary completions powershell | Out-String | Invoke-Expression` to
`$PROFILE`. Where running `locallibrary` on Tab isn't wanted, `completions SHELL --static`
prints a script that completes only commands and flags. `locallibrary man -o DIR` writes a man
page for every command (`locallibrary.1`, `locallibrary-drive-ls.1`...) into `DIR`, for a
`man` path such as `/usr/local/share/man/man1`; `locallibrary man` prints the top-level one.

## Platforms

locallibrary is developed and used on macOS. **Linux and Windows are experimental, untested on
hardware**, and CI hasn't passed on them yet. Most commands only read and write files, and are
written to work the same on all three. The commands that drive the operating system's own disk tools
have only ever run on macOS: `drive format` (with `--check-capacity` too, and `provision --format`),
`drive fsck`, `eject` (and `--eject` on `sync`, `library push` and `provision`, and `prep`) and
`drive rename --label`. On Linux and Windows their code paths have never run (the Windows FAT32
writer hasn't even been compiled), so their plans carry a warning (the sidecar and the terminal
player show it too; `drive fsck`'s report has it), and you should check the result yourself
(`locallibrary drive disks`, `locallibrary drive ls`) before trusting a drive to it.
