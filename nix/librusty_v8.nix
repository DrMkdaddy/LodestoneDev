# auto-generated file -- DO NOT EDIT!
{
  stdenv,
  fetchurl,
}: let
  fetch_librusty_v8 = args:
    fetchurl {
      name = "librusty_v8-${args.version}";
      url = "https://github.com/denoland/rusty_v8/releases/download/v${args.version}/librusty_v8_release_${stdenv.hostPlatform.rust.rustcTarget}.a";
      sha256 = args.shas.${stdenv.hostPlatform.system};
      meta = {inherit (args) version;};
    };
in
  fetch_librusty_v8 {
    version = "0.73.0";
    shas = {
      x86_64-linux = "sha256-rDthrqAs4yUl9BpFm8yJ2sKbUImydMMZegUBhcu6vdk=";
      aarch64-linux = "sha256-NwuDVaEbYWZfF2epVPPhIKM1aw35e8Bp5siBoREpym8=";
      x86_64-darwin = "sha256-JNf4Me4VMdvjgYM4OyCUlQW6Q/OdQ2BuDNWFKI4sbtg=";
      aarch64-darwin = "sha256-WVBDiGWVBvMiKNgPaPw8KQChARkFDfx8Nt9QEobGNG4=";
    };
  }
