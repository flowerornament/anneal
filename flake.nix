{
  description = "Convergence assistant for knowledge corpora";
  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay/afcefd09c71e63663a46e4ce5e09968584f2c709";
  inputs.rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  nixConfig = {
    extra-substituters = [ "https://flowerornament.cachix.org" ];
    extra-trusted-public-keys = [
      "flowerornament.cachix.org-1:gSODgIXgfRANrEGITBOF8XWaEKNy8hkNGfRVwqUG46c="
    ];
  };
  outputs = { self, nixpkgs, rust-overlay }:
    let
      annealVersion = "0.29.1";
      systems = [ "aarch64-darwin" "x86_64-darwin" "aarch64-linux" "x86_64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          rustPkgs = pkgs.extend rust-overlay.overlays.default;
          toolchain = rustPkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          rustPlatform = pkgs.makeRustPlatform { cargo = toolchain; rustc = toolchain; };
        in f { inherit pkgs rustPlatform; });
    in {
      packages = forAllSystems ({ pkgs, rustPlatform }: {
        default = import ./nix/package.nix {
          inherit pkgs rustPlatform;
          src = ./.;
          version = annealVersion;
        };
      });

      homeManagerModules.default = import ./nix/home-manager.nix {
        inherit self;
        src = ./.;
      };

      # Source tree path for skill syncing (nix-config agent-sync.nix).
      skillsDir = "${self}/skills";
    };
}
