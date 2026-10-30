{ pkgs, rustPlatform, src, version }:
rustPlatform.buildRustPackage {
  pname = "anneal";
  inherit version src;
  cargoLock = {
    lockFile = src + /Cargo.lock;
    outputHashes = { "atlas-core-0.1.1" = "sha256-ywlv91IWhEW/wk3dayUQxNtGKulYHHPRv0dG+VTHAe0="; };
  };
  nativeCheckInputs = [ pkgs.git pkgs.jujutsu ];
  ANNEAL_REQUIRE_JJ = "1";
  meta = {
    description = "Convergence assistant for knowledge corpora";
    license = pkgs.lib.licenses.mit;
    mainProgram = "anneal";
  };
}
