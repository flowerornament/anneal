{ src, annealVersion }:
{ config, lib, pkgs, ... }:
let
  defaultPackage = import ./package.nix {
    inherit pkgs;
    inherit src;
    version = annealVersion;
  };
  isRelativeSkillTarget = target: target != "" && builtins.substring 0 1 target != "/";
in
{
  options.programs.anneal = {
    enable = lib.mkEnableOption "anneal convergence assistant";

    package = lib.mkOption {
      type = lib.types.package;
      default = defaultPackage;
      defaultText = lib.literalExpression "anneal package from this flake";
      description = "The anneal package to install.";
    };

    settings = {
      state = {
        historyMode = lib.mkOption {
          type = lib.types.nullOr (lib.types.enum [ "xdg" "repo" "off" ]);
          default = null;
          example = "xdg";
          description = ''
            Optional override for anneal's history backend mode in user config.
            This maps to `state.history_mode` in `anneal/config.toml`.
          '';
        };

        historyDir = lib.mkOption {
          type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
          default = null;
          example = lib.literalExpression ''"/Users/alice/.local/state"'';
          description = ''
            Optional base directory for anneal's machine-local derived history.
            This maps to `state.history_dir` in `anneal/config.toml`.
          '';
        };
      };
    };

    skill = {
      enable = lib.mkEnableOption "anneal skill symlink management";

      targets = lib.mkOption {
        type = lib.types.listOf (lib.types.addCheck lib.types.str isRelativeSkillTarget);
        default = [ ".agents/skills/anneal" ];
        example = lib.literalExpression ''
          [
            ".agents/skills/anneal"
            ".codex/skills/anneal"
          ]
        '';
        description = ''
          Home-relative paths where Home Manager should symlink anneal's
          `skills/anneal` directory when skill installation is enabled.
        '';
      };
    };
  };

  config =
    let
      cfg = config.programs.anneal;
      hasStateConfig =
        cfg.settings.state.historyMode != null
        || cfg.settings.state.historyDir != null;
      hasUniqueSkillTargets = builtins.length cfg.skill.targets == builtins.length (lib.unique cfg.skill.targets);
      userConfigText =
        lib.concatStringsSep "\n" (
          [ "[state]" ]
          ++ lib.optional (cfg.settings.state.historyMode != null)
            "history_mode = ${builtins.toJSON cfg.settings.state.historyMode}"
          ++ lib.optional (cfg.settings.state.historyDir != null)
            "history_dir = ${builtins.toJSON (toString cfg.settings.state.historyDir)}"
        )
        + "\n";
      skillSource = "${src}/skills/anneal";
      skillFiles = lib.genAttrs cfg.skill.targets (_: {
        source = skillSource;
      });
    in
    lib.mkMerge [
      {
        assertions = [
          {
            assertion = !cfg.skill.enable || cfg.enable;
            message = "programs.anneal.skill.enable requires programs.anneal.enable = true";
          }
          {
            assertion = !cfg.skill.enable || hasUniqueSkillTargets;
            message = "programs.anneal.skill.targets must not contain duplicate paths";
          }
        ];
      }
      (lib.mkIf cfg.enable {
        home.packages = [ cfg.package ];
      })
      (lib.mkIf (cfg.enable && hasStateConfig) {
        xdg.configFile."anneal/config.toml".text = userConfigText;
      })
      (lib.mkIf (cfg.enable && cfg.skill.enable) {
        home.file = skillFiles;
      })
    ];
}
