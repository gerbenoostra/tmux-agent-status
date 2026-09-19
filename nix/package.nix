{
  lib,
  rustPlatform,
  tmux,
  unixtools,
}:

rustPlatform.buildRustPackage {
  pname = "tmux-agent-status";
  version = (lib.importTOML ../Cargo.toml).package.version;

  src = lib.cleanSource ../.;

  # The lock file rather than a vendor hash: this is a binary crate whose
  # Cargo.lock is committed, so there is nothing to regenerate on a bump.
  cargoLock.lockFile = ../Cargo.lock;

  # Half the suite drives a real tmux server, which needs a real, writable
  # $HOME to start against - the Linux sandbox's placeholder `/homeless-shelter`
  # exists (so `cd` into it works, if uselessly), but the Darwin sandbox never
  # creates it at all, and `tmux new-session` fails outright with a cwd that
  # does not exist. Verified: on aarch64-darwin, `nix build .#checks` fails
  # every `register_probe` test with `spawn` returning ENOENT until `$HOME` is
  # a real directory.
  #
  # The lock ownership tests shell out to `ps` and `hostname`, which the build
  # sandbox's minimal $PATH does not otherwise carry. `unixtools` picks the
  # right implementation per platform (the real `ps`/`hostname` on Darwin,
  # `procps`/`inetutils` on Linux).
  nativeCheckInputs = [
    tmux
    unixtools.hostname
    unixtools.ps
  ];
  preCheck = ''
    export HOME=$(mktemp -d)
  '';

  postInstall = ''
    install -Dm644 share/tmux/tmux-agent-status.conf \
      $out/share/tmux/tmux-agent-status.conf
    for f in share/agents/*/*; do
      install -Dm644 "$f" "$out/$f"
    done
  '';

  meta = {
    description = "Agent lifecycle events as one glyph on the tmux window entry";
    homepage = "https://github.com/gerbenoostra/tmux-agent-status";
    license = lib.licenses.mit;
    mainProgram = "tmux-agent-status";
    platforms = lib.platforms.unix;
  };
}
