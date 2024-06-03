{
  description = "A free, open source server hosting tool for Minecraft and other multiplayers.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {flake-parts, ...}:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux" "aarch64-linux"];

      perSystem = {
        config,
        self',
        inputs',
        pkgs,
        system,
        rust-overlay,
        nixpkgs,
        ...
      }: let
        overlays = [(import inputs.rust-overlay)];
        pkgs = import inputs.nixpkgs {
          inherit system overlays;
        };
        inherit (inputs.nixpkgs) lib;
        inherit (lib) getExe;
      in {
        formatter = pkgs.alejandra;

        devShells.default = pkgs.mkShell {
          inputsFrom = builtins.attrValues self'.packages;

          packages = with pkgs; [
            alejandra # nix formatter
            statix # lints and suggestions
            deadnix # clean up unused nix code
            gcc
            (rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
          ];
        };
        packages = let
          lockFile = ./Cargo.lock;
        in rec {
          lodestone = pkgs.callPackage ./nix/default.nix {inherit inputs lockFile;};
          default = lodestone;
        };
      };
      flake = _: {
        nixosModules = rec {
          lodestone = import ./nix/module.nix inputs.self;
          default = lodestone;
        };
      };
    };
}
