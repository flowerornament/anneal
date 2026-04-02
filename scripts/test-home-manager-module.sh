#!/usr/bin/env bash
set -euo pipefail

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || {
        printf 'error: missing required command: %s\n' "$1" >&2
        exit 1
    }
}

require_cmd nix
require_cmd python3
require_cmd git

ROOT="$(git rev-parse --show-toplevel)"
TMPDIR="$(python3 - <<'PY'
import os
import tempfile

print(os.path.realpath(tempfile.mkdtemp()))
PY
)"
trap 'rm -rf "$TMPDIR"' EXIT

configured_json="$TMPDIR/configured.json"
bare_json="$TMPDIR/bare.json"

nix eval --impure --json --expr "
let
  flake = builtins.getFlake \"path:${ROOT}\";
  pkgs = import flake.inputs.nixpkgs { system = builtins.currentSystem; };
  lib = flake.inputs.nixpkgs.lib;
  module = flake.outputs.homeManagerModules.default;
  stub = { lib, ... }: {
    options.assertions = lib.mkOption {
      type = lib.types.listOf lib.types.attrs;
      default = [ ];
    };
    options.home.packages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
    };
    options.home.file = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule {
        options.source = lib.mkOption {
          type = lib.types.path;
        };
      });
      default = { };
    };
    options.xdg.configFile = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule {
        options.text = lib.mkOption {
          type = lib.types.lines;
        };
      });
      default = { };
    };
  };
  evaluated = lib.evalModules {
    modules = [
      module
      stub
      {
        programs.anneal.enable = true;
        programs.anneal.settings.state.historyMode = \"xdg\";
        programs.anneal.settings.state.historyDir = \"/tmp/anneal-state\";
        programs.anneal.skill.enable = true;
        programs.anneal.skill.targets = [
          \".agents/skills/anneal-test\"
          \".codex/skills/anneal-test\"
        ];
      }
    ];
    specialArgs = { inherit pkgs; };
  };
in {
  hasFile = evaluated.config.xdg.configFile ? \"anneal/config.toml\";
  text = evaluated.config.xdg.configFile.\"anneal/config.toml\".text;
  hasAgentsSkill = evaluated.config.home.file ? \".agents/skills/anneal-test\";
  hasCodexSkill = evaluated.config.home.file ? \".codex/skills/anneal-test\";
  agentsSkillSource = evaluated.config.home.file.\".agents/skills/anneal-test\".source;
  codexSkillSource = evaluated.config.home.file.\".codex/skills/anneal-test\".source;
  packageCount = builtins.length evaluated.config.home.packages;
}
" > "$configured_json"

nix eval --impure --json --expr "
let
  flake = builtins.getFlake \"path:${ROOT}\";
  pkgs = import flake.inputs.nixpkgs { system = builtins.currentSystem; };
  lib = flake.inputs.nixpkgs.lib;
  module = flake.outputs.homeManagerModules.default;
  stub = { lib, ... }: {
    options.assertions = lib.mkOption {
      type = lib.types.listOf lib.types.attrs;
      default = [ ];
    };
    options.home.packages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
    };
    options.home.file = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule {
        options.source = lib.mkOption {
          type = lib.types.path;
        };
      });
      default = { };
    };
    options.xdg.configFile = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule {
        options.text = lib.mkOption {
          type = lib.types.lines;
        };
      });
      default = { };
    };
  };
  evaluated = lib.evalModules {
    modules = [
      module
      stub
      {
        programs.anneal.enable = true;
      }
    ];
    specialArgs = { inherit pkgs; };
  };
in {
  hasFile = evaluated.config.xdg.configFile ? \"anneal/config.toml\";
  hasAgentsSkill = evaluated.config.home.file ? \".agents/skills/anneal\";
  packageCount = builtins.length evaluated.config.home.packages;
}
" > "$bare_json"

python3 - <<'PY' "$configured_json" "$bare_json"
import json
import pathlib
import sys

configured = json.loads(pathlib.Path(sys.argv[1]).read_text())
bare = json.loads(pathlib.Path(sys.argv[2]).read_text())

if not configured.get("hasFile"):
    raise SystemExit("configured case did not emit anneal/config.toml")

if bare.get("hasFile"):
    raise SystemExit("bare case unexpectedly emitted anneal/config.toml")

if not configured.get("hasAgentsSkill"):
    raise SystemExit("configured case did not emit the anneal skill for ~/.agents")

if not configured.get("hasCodexSkill"):
    raise SystemExit("configured case did not emit the anneal skill for ~/.codex")

if bare.get("hasAgentsSkill"):
    raise SystemExit("bare case unexpectedly emitted the anneal skill")

configured_package_count = configured["packageCount"]
if configured_package_count < 1:
    raise SystemExit("configured case did not add anneal to home.packages")

bare_package_count = bare["packageCount"]
if bare_package_count < 1:
    raise SystemExit("bare case did not add anneal to home.packages")

content = configured["text"]

expected_lines = [
    "[state]",
    'history_mode = "xdg"',
    'history_dir = "/tmp/anneal-state"',
]
for line in expected_lines:
    if line not in content:
        raise SystemExit(f"generated config missing line: {line!r}\n{content}")

agents_skill_source = pathlib.Path(configured["agentsSkillSource"])
codex_skill_source = pathlib.Path(configured["codexSkillSource"])

for skill_source in (agents_skill_source, codex_skill_source):
    if not (skill_source / "SKILL.md").is_file():
        raise SystemExit(f"skill source missing SKILL.md: {skill_source}")

print("configured_text=true")
print(f"configured_agents_skill_source={agents_skill_source}")
print(f"configured_codex_skill_source={codex_skill_source}")
print("--- configured file ---")
print(content.rstrip())
print("--- assertions ---")
print("configured_has_file=true")
print("bare_has_file=false")
print("configured_has_agents_skill=true")
print("configured_has_codex_skill=true")
print("bare_has_agents_skill=false")
print(f"configured_package_count={configured_package_count}")
print(f"bare_package_count={bare_package_count}")
PY

printf 'Home Manager module smoke test passed.\n'
