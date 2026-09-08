{
  description = "agent-status: agent lifecycle events as one glyph on the tmux window entry";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        agent-status = pkgs.callPackage ./nix/package.nix { };
        default = agent-status;
      });

      checks = forAllSystems (pkgs: {
        agent-status = self.packages.${pkgs.system}.agent-status;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.clippy
            pkgs.rustfmt
            pkgs.rust-analyzer
            pkgs.tmux
            pkgs.just
          ];
        };
      });
    };
}
