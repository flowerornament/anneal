{ src, annealVersion }:
{ config, lib, pkgs, ... }:
let
  tomlFormat = pkgs.formats.toml { };
  defaultPackage = import ./package.nix {
    inherit pkgs;
    inherit src;
    version = annealVersion;
  };
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
  };

  config =
    let
      cfg = config.programs.anneal;
      stateConfig =
        (lib.optionalAttrs (cfg.settings.state.historyMode != null) {
          history_mode = cfg.settings.state.historyMode;
        })
        // (lib.optionalAttrs (cfg.settings.state.historyDir != null) {
          history_dir = toString cfg.settings.state.historyDir;
        });

      userConfig = lib.optionalAttrs (stateConfig != { }) {
        state = stateConfig;
      };
    in
    lib.mkIf cfg.enable (
      {
        home.packages = [ cfg.package ];
      }
      // lib.optionalAttrs (userConfig != { }) {
        xdg.configFile."anneal/config.toml".source =
          tomlFormat.generate "anneal-config.toml" userConfig;
      }
    );
}
