self: {
  config,
  pkgs,
  lib,
  ...
}: let
  cfg = config.programs.lodestone;
  defaultPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
in {
  meta.maintainters = with lib.maintainters; [DrMkdaddy];

  options.programs.lodestone = with lib; {
    enable = mkEnableOption "lodestone";

    package = mkOption {
      type = with types; nullOr package;
      default = defaultPackage;
      defaultText = lib.literalExpression ''
        lodestone.packages.''${pkgs.stdenv.hostPlatform.system}.default
      '';
      description = mdDoc ''
        Lodestone package to use. Defaults to the one provided by the flake.
      '';
    };
  };
}
