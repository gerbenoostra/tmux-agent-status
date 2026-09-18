# 012 - One-liner `curl ... | sh` binary install

Status: done on `feat/binary-install`. `install.sh` added at the repository root and documented in
`docs/install.md` and `README.md`. Verified against a synthetic fixture matching the real release
layout (happy path, tampered checksum, missing checksum tool, unsafe archive paths, sed non-match
on the redirect lookup, API-fallback failure) since the repository has not published a tagged
release yet; `shellcheck -s sh` is clean. The true end-to-end path (a real tagged release through
`curl ... | sh`) remains unverified until the first release is published.

## Goal

Add a one-line shell installer that downloads and installs the `tmux-agent-status` binary, so a user can run:

```sh
curl -fsSL <release-url> | sh
```

and have `tmux-agent-status` on `PATH`.

## Scope

- Detect platform and architecture.
- Download a matching release archive.
- Verify checksums when available.
- Place the binary in `~/.local/bin` or another standard location.
- Add the install path to `PATH` if needed, with instructions.

## Notes

- Keep the install script in the repository root as `install.sh`.
- `docs/install.md` already covers manual install; this is an automated alternative.
- The `install` subcommand (plan 011) stays config-only; this todo is for the binary only.
