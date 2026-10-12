# The fonts the web build serves from its own site, fetched by the app only when a workbook uses
# them (crates/ui-egui/src/font_offer.rs). Every family is under the SIL Open Font License 1.1 or
# Apache-2.0 and ships with its licence. Only static faces: egui draws a variable font's default
# instance only, so bold and italic need their own files. Aliases name the fonts each one stands
# in for with matching metrics (Carlito for Calibri, Liberation Sans for Arial, ...), so a workbook
# set in them keeps its layout in the browser without any vendor font.
{ pkgs }:
let
  lib = pkgs.lib;
  four = regular: bold: italic: boldItalic: [
    {
      file = regular;
      weight = 400;
      italic = false;
    }
    {
      file = bold;
      weight = 700;
      italic = false;
    }
    {
      file = italic;
      weight = 400;
      italic = true;
    }
    {
      file = boldItalic;
      weight = 700;
      italic = true;
    }
  ];
  # A family whose faces are named Stem-Regular, -Bold, -Italic and -BoldItalic.
  standard = stem: four "${stem}-Regular.ttf" "${stem}-Bold.ttf" "${stem}-Italic.ttf" "${stem}-BoldItalic.ttf";
  # The Carlito tarball carries its OFL text; the package does not install it.
  carlitoLicense = pkgs.runCommand "carlito-license" { } ''
    tar -xzOf ${pkgs.carlito.src} --wildcards '*/LICENSE' > "$out"
  '';
  ttf = pkg: "${pkg}/share/fonts/truetype";
  families = [
    {
      family = "Carlito";
      aliases = [ "Calibri" ];
      dir = "Carlito";
      from = ttf pkgs.carlito;
      faces = standard "Carlito";
      license = carlitoLicense;
    }
    {
      family = "Caladea";
      aliases = [ "Cambria" ];
      dir = "Caladea";
      from = ttf pkgs.caladea;
      faces = standard "Caladea";
      # Apache-2.0; the upstream tarball has no licence file, so ship the licence text itself.
      license = ../LICENSE-APACHE;
    }
    {
      family = "Liberation Sans";
      aliases = [
        "Arial"
        "Helvetica"
      ];
      dir = "LiberationSans";
      from = ttf pkgs.liberation_ttf;
      faces = standard "LiberationSans";
      license = "${pkgs.liberation_ttf.src}/LICENSE";
    }
    {
      family = "Liberation Serif";
      aliases = [
        "Times New Roman"
        "Times"
      ];
      dir = "LiberationSerif";
      from = ttf pkgs.liberation_ttf;
      faces = standard "LiberationSerif";
      license = "${pkgs.liberation_ttf.src}/LICENSE";
    }
    {
      family = "Liberation Mono";
      aliases = [
        "Courier New"
        "Courier"
      ];
      dir = "LiberationMono";
      from = ttf pkgs.liberation_ttf;
      faces = standard "LiberationMono";
      license = "${pkgs.liberation_ttf.src}/LICENSE";
    }
    {
      family = "Gelasio";
      aliases = [ "Georgia" ];
      dir = "Gelasio";
      from = ttf pkgs.gelasio;
      faces = standard "Gelasio";
      license = "${pkgs.gelasio.src}/OFL.txt";
    }
    {
      family = "Lato";
      aliases = [ ];
      dir = "Lato";
      from = "${pkgs.lato}/share/fonts/lato";
      faces = standard "Lato";
      license = "${pkgs.lato.src}/Lato2OFL/OFL.txt";
    }
    {
      family = "Source Sans 3";
      aliases = [ ];
      dir = "SourceSans3";
      from = ttf pkgs.source-sans;
      faces = four "SourceSans3-Regular.ttf" "SourceSans3-Bold.ttf" "SourceSans3-It.ttf" "SourceSans3-BoldIt.ttf";
      license = "${pkgs.source-sans.src}/LICENSE.md";
    }
    {
      family = "Merriweather";
      aliases = [ ];
      dir = "Merriweather";
      from = ttf pkgs.merriweather;
      faces = standard "Merriweather";
      license = "${pkgs.merriweather.src}/OFL.txt";
    }
  ];
  licenseName = f: "${f.dir}/LICENSE.txt";
  manifest = pkgs.writeText "manifest.json" (
    builtins.toJSON {
      families = map (f: {
        inherit (f) family aliases;
        license = licenseName f;
        faces = map (face: face // { file = "${f.dir}/${face.file}"; }) f.faces;
      }) families;
    }
  );
  copyFamily = f: ''
    mkdir -p "$out/${f.dir}"
    install -m 444 ${f.license} "$out/${licenseName f}"
    ${lib.concatMapStrings (face: ''
      install -m 444 "${f.from}/${face.file}" "$out/${f.dir}/${face.file}"
    '') f.faces}
  '';
in
pkgs.runCommand "gridcraft-web-fonts" { } ''
  ${lib.concatMapStrings copyFamily families}
  install -m 444 ${manifest} "$out/manifest.json"
''
