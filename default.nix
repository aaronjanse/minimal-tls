with import <nixpkgs> {};

stdenv.mkDerivation rec {
  name = "minimal-tls";
  buildInputs = [ rustup clang libsodium.dev openssl rust-bindgen boringssl ];
  LIBCLANG_PATH="${llvmPackages.libclang}/lib";

  nativeBuildInputs = [ pkgconfig ];

  SODIUM_USE_PKG_CONFIG = "yes";
}
