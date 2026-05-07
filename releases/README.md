# Releases

Release notes for tagged Den versions. Each `vX.Y.Z.md` file is the body of
the corresponding GitHub Release published on
[`bearocratic/den`](https://github.com/bearocratic/den).

## How to cut a release

From a clean `main` working tree:

```sh
scripts/release.sh vX.Y.Z
```

That script bumps `Cargo.toml`, refreshes `Cargo.lock`, copies
`TEMPLATE.md` to `releases/vX.Y.Z.md`, opens it in `$EDITOR` for you to
fill in, then commits (`chore(release): vX.Y.Z`), tags, and pushes.

The `Release` workflow then takes over: validates the tag and notes,
builds binaries for darwin-arm64/amd64 and linux-amd64/arm64, publishes a
GitHub Release, and bumps the formula in `bearocratic/homebrew-tap`.

## Conventions

- One file per tag. Filename matches the tag exactly: `v0.1.0.md`, not
  `0.1.0.md`.
- Tag format is strict `vX.Y.Z`. Pre-releases and metadata suffixes are not
  wired through the workflow.
- `Cargo.toml`'s `version` must equal the tag (without the `v`). The
  workflow refuses to release if they disagree.
- Notes are written for end users — what they can now do, what changed,
  what broke. Internal refactors don't need a section.
