{
  description = "Convergence assistant for knowledge corpora";
  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }:
    let
      systems = [ "aarch64-darwin" "x86_64-darwin" "aarch64-linux" "x86_64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f {
        pkgs = nixpkgs.legacyPackages.${system};
      });
    in {
      packages = forAllSystems ({ pkgs }: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "anneal";
          version = "0.3.1";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          meta = {
            description = "Convergence assistant for knowledge corpora";
            license = pkgs.lib.licenses.mit;
            mainProgram = "anneal";
          };
        };
      });

      homeManagerModules.default = import ./nix/home-manager.nix { inherit self; };

      # Source tree path for skill syncing (nix-config agent-sync.nix).
      skillsDir = "${self}/skills";
    };
}
