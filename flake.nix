{
  description = "Convergence assistant for knowledge corpora";
  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  nixConfig = {
    extra-substituters = [ "https://flowerornament.cachix.org" ];
    extra-trusted-public-keys = [
      "flowerornament.cachix.org-1:gSODgIXgfRANrEGITBOF8XWaEKNy8hkNGfRVwqUG46c="
    ];
  };
  outputs = { self, nixpkgs }:
    let
      annealVersion = "0.25.1";
      systems = [ "aarch64-darwin" "x86_64-darwin" "aarch64-linux" "x86_64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f {
        pkgs = nixpkgs.legacyPackages.${system};
      });
    in {
      packages = forAllSystems ({ pkgs }: {
        default = import ./nix/package.nix {
          inherit pkgs;
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
