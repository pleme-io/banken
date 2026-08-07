{
  description = "banken 番犬 — the pleme-io-native k9s: an observe-first, GitOps-native cluster-navigator TUI";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-25.11";
    crate2nix.url = "github:nix-community/crate2nix";
    flake-utils.url = "github:numtide/flake-utils";
    substrate = {
      url = "github:pleme-io/substrate";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    crate2nix,
    flake-utils,
    substrate,
  }:
    (import "${substrate}/lib/rust-tool-release-flake.nix" {
      inherit nixpkgs crate2nix flake-utils;
    }) {
      toolName = "banken";
      # The workspace holds three crates: `banken-spec` (the postigo
      # TYPED-SPEC + interpreter triplet, downstream-consumable),
      # `banken-config` (the shikumi/tatara config surface, also
      # downstream-consumable) and `banken` (the bin).
      # packageName pins substrate's crateKey to the crate that owns the
      # binary, exactly as tobira does for `tobirato`.
      packageName = "banken";
      src = self;
      repo = "pleme-io/banken";

      # NOTE on features (corrected 2026-08-07): this used to say the Nix
      # build was the FIXTURE path, with `live` unreachable from Nix because
      # substrate exposes no per-consumer feature knob. The second half is
      # still true — substrate's tool-release shape takes its feature set
      # from gen's build spec (cargo's *default* resolve), and there is no
      # `rootFeatures` on this shape — but the conclusion was wrong. The knob
      # that WAS available is the one this crate owns: `default`. banken's
      # Cargo.toml now declares `default = ["live"]`, so `packages.default`
      # (and therefore `pkgs.banken` on every fleet node) carries the
      # KubeClusterEnv read. See banken/Cargo.toml's `[features]` block for
      # why `tear` deliberately did NOT come along.
    };
}
