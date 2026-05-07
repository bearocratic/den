# Den

A terminal watcher for the git repositories in a folder. Tiles per repo show
dirty state at a glance — staged, modified, untracked, conflicts, ahead /
behind, latest tag, README, release notes — without leaving the terminal.

## Install

```sh
brew tap bearocratic/tap
brew install den
```

Upgrade later with `brew upgrade den`.

## Usage

```sh
den                   # scan the current directory
den ~/Projects        # scan a specific folder
den --depth 6 .       # change recursion depth (default 4)
```

Press `:` to open the command palette. Every shortcut, with a description,
in one place. Pins (`p`), hides (`x`) and the show-hidden toggle (`.`)
persist to `~/.config/den/`.

## Optional integrations

- `lazygit` — press `o` on a tile to drop into lazygit on that repo. Den
  suspends, lazygit takes over, Den resumes when you quit. If `lazygit` is
  not on `$PATH`, the action surfaces an error and otherwise no-ops.
- `$EDITOR` / `$VISUAL` — `e` opens the selected repo there (falls back to
  `code`).
- System browser — `g` opens the repo's GitHub page if `origin` is set.

## Platforms

- macOS (Apple Silicon, Intel)
- Linux (amd64, arm64)

## Releases

Tagged builds and changelogs live under
[Releases](https://github.com/bearocratic/den/releases). Each release ships
prebuilt binaries; the Homebrew formula at
[`bearocratic/homebrew-tap`](https://github.com/bearocratic/homebrew-tap)
tracks the latest tag.

## License

Proprietary. See `LICENSE`.
