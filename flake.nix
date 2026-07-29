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

      # NOTE on features: banken's Cargo.toml declares `default = []`, so
      # this builds the FIXTURE path (BANKEN.md §VI M0, proven green). The
      # live-cluster read sits behind the `live` cargo feature (kube +
      # k8s-openapi + rustls — a heavy tree we do not force on consumers).
      # Substrate exposes no per-consumer feature knob today, so selecting
      # `live` from Nix is `pending-banken: live-read` alongside the live
      # read itself; `cargo build --features live` is the path meanwhile.
    };
}
