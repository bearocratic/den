# Den

A terminal watcher for the git repositories in a folder. Tiles per repo show
dirty state at a glance — staged, modified, untracked, conflicts, ahead /
behind, stash count, latest tag, and CI status — without leaving the
terminal. Background `git fetch` keeps ahead/behind counts live.

## Install

```sh
brew tap bearocratic/tap
brew install den
```

Upgrade later with `brew upgrade den`.

## Usage

```sh
den                              # scan the current directory
den ~/bearocratic                # scan a specific folder
den --depth 6 ~/bearocratic      # change recursion depth (default 4)
den --fetch-interval 60 .        # fetch every 60s (0 disables)
den --no-ci .                    # skip `gh` calls
```

Press `:` to open the command palette. Every shortcut, with a description,
in one place. Pins (`p`), hides (`x`) and the show-hidden toggle (`.`)
persist to `~/.config/den/`.

## Keys

| Key | Action |
|-----|--------|
| `↑↓←→` / `hjkl` | Move selection |
| `↵` | Toggle detail pane |
| `1` / `2` | Focus status / diff |
| `/` | Filter tiles by name |
| `:` | Open command palette |
| `p` / `x` | Pin / hide focused repo |
| `e` / `o` / `s` / `g` / `A` | Open in editor / lazygit / shell / GitHub / GitHub Actions |
| `r` / `F` | Refresh all / fetch focused now |
| `i` | Toggle README overlay |
| `q` | Quit |

## Optional integrations

- `lazygit` — `o` drops into lazygit on the focused repo. Den suspends,
  lazygit takes over, Den resumes when you quit.
- `$SHELL` — `s` drops into a shell at the repo path with the same
  suspend/resume.
- `$EDITOR` / `$VISUAL` — `e` opens the selected repo there (falls back to
  `code`).
- System browser — `g` opens the repo's GitHub page if `origin` is set;
  `A` opens the GitHub Actions tab.
- `gh` CLI — required for CI status badges. If `gh auth login` hasn't been
  run, badges stay blank and Den prints a hint at startup. Disable with
  `--no-ci`.

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
