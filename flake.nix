# Dev shell for the coach backend (Rust). Enter with: nix develop
# Pure-Rust TLS (rustls) so there's no openssl/pkg-config native dep.
{
  description = "coach — periodized training tracker + pacing coach";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "aarch64-darwin" "x86_64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in {
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rust-analyzer
            pkgs.rustfmt
            pkgs.clippy
            pkgs.sqlx-cli
            pkgs.nodejs_24 # Angular 22 frontend (frontend/)
            pkgs.pnpm # the frontend's installer; node ships npm too, ignore it
          ];
          # Playwright's browsers come from the lock, not ~/Library/Caches: the
          # driver's version must match @playwright/test's (tables/deps.dhall).
          PLAYWRIGHT_BROWSERS_PATH = pkgs.playwright-driver.browsers;
          PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "1";
        };
      });
    };
}
