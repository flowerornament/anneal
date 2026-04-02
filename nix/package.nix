{ pkgs, src, version }:
pkgs.rustPlatform.buildRustPackage {
  pname = "anneal";
  inherit version src;
  cargoLock.lockFile = src + /Cargo.lock;
  meta = {
    description = "Convergence assistant for knowledge corpora";
    license = pkgs.lib.licenses.mit;
    mainProgram = "anneal";
  };
}
