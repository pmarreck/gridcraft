# Nixpkgs's LLVM winpthreads build uses a no-libc compiler; llvm-windres
# consequently cannot preprocess version.rc's <winver.h>. The Windows ARM64
# package build is the regression probe. Remove this override once Nixpkgs's
# resource-compiler wrapper supplies the target header search path itself.
final: prev: {
  windows = prev.windows.overrideScope (
    wfinal: wprev: {
      pthreads =
        if prev.stdenv.hostPlatform.isAarch64 && prev.stdenv.hostPlatform.isWindows then
          wprev.pthreads.overrideAttrs (_: {
            RCFLAGS = "-I${wfinal.mingw_w64_headers}/include";
          })
        else
          wprev.pthreads;
    }
  );
}
