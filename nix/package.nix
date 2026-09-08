{
  lib,
  rustPlatform,
  tmux,
}:

rustPlatform.buildRustPackage {
  pname = "agent-status";
  version = (lib.importTOML ../Cargo.toml).package.version;

  src = lib.cleanSource ../.;

  # The lock file rather than a vendor hash: this is a binary crate whose
  # Cargo.lock is committed, so there is nothing to regenerate on a bump.
  cargoLock.lockFile = ../Cargo.lock;

  # Half the suite drives a real tmux server.
  nativeCheckInputs = [ tmux ];

  postInstall = ''
    install -Dm644 share/tmux/agent-status.conf \
      $out/share/tmux/agent-status.conf
  '';

  meta = {
    description = "Agent lifecycle events as one glyph on the tmux window entry";
    homepage = "https://github.com/gerbenoostra/tmux-agent-status";
    license = lib.licenses.mit;
    mainProgram = "agent-status";
    platforms = lib.platforms.unix;
  };
}
