{
  description = "haw — reproducible multi-repo stacks + cross-repo PR/MR orchestration, with a k9s-style TUI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "hawser";
          version = "0.1.1";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          # Build only the `haw` binary from the workspace.
          cargoBuildFlags = [ "-p" "hawser" ];
          # Tests touch git/network; skip them in the sandboxed build.
          doCheck = false;

          nativeBuildInputs = [ pkgs.makeWrapper ];

          # haw shells out to `git` for mutations at runtime.
          postInstall = ''
            wrapProgram $out/bin/haw --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.git ]}
          '';

          meta = with pkgs.lib; {
            description = "Reproducible multi-repo stacks + cross-repo PR/MR orchestration (haw)";
            homepage = "https://github.com/Nastwinns/hawser";
            license = with licenses; [ mit asl20 ];
            mainProgram = "haw";
          };
        };

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/haw";
        };
      });
}
