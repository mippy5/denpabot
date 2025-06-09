{
  lib,
  rustPlatform,
  pkg-config,
}:
rustPlatform.buildRustPackage {
  pname = "denpabot";
  version = "0.1.0";

  src = ../.;

  cargoDeps = rustPlatform.importCargoLock {
    lockFile = ../Cargo.lock;
  };

  meta = with lib; {
    homepage = "https://github.com/mippy5/denpabot";
    mainProgram = "denpabot";
    platforms = platforms.unix;
  };
}
