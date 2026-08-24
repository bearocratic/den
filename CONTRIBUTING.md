# Contributing to Den

Den is open source under the [Apache License 2.0](LICENSE) and lives in
this single repository. Contributions are welcome — and the tool stays
small on purpose, so the bar for new surface area is deliberately high.

## Pull requests

- **Open an issue first** for anything bigger than a typo or an obvious
  bug fix. Den is small on purpose; agreeing on the shape before you
  write the code saves everyone a revert.
- **Sign off every commit** (`git commit -s`). Den uses the
  [Developer Certificate of Origin](https://developercertificate.org/) —
  the sign-off certifies you have the right to submit the change.
  Contributions land under Apache-2.0, the same license Den ships under.
- **Keep it tight.** One change per PR, no drive-by refactors, follow
  the style around you.
- The maintainer has final say. A well-made PR can still be declined if
  it grows Den in a direction the tool shouldn't go — the issue-first
  rule exists to catch that early.

## Bug reports and feature requests

- **Bug reports.** Use the [Bug report](.github/ISSUE_TEMPLATE/bug-report.yml)
  template. The more specific, the better — Den version, platform, terminal,
  the shape of your repos folder, the keys you pressed.
- **Feature requests.** Use the
  [Feature request](.github/ISSUE_TEMPLATE/feature-request.yml) template.
  Describe the situation first, the proposed feature second.
- **Security reports.** Please don't open a public issue. Use
  [GitHub's private security advisory flow](https://github.com/bearocratic/den/security/advisories/new),
  or write to `security@bearocratic.io`.

## Trademarks

The code is Apache-2.0; the name is not. "Den" and the Bearocratic bear
remain Bearocratic OÜ's marks — fork freely, but ship your fork under
your own name.

## Why the tight bar

Den is small on purpose. Keeping the surface area tight — both code and
behaviour — is the easiest way to keep it from drifting into something
nobody asked for. Issues are how the tool listens; releases are how it
answers.

Thank you for being here.
