[
  # (fetchurl "url") needs to immediately fetch, but our options without
  # internet access are fairly limited.
  # TODO: populate some fixtures at a known location instead.
  (builtins.fetchurl "file:///dev/null")

  # fetchurl with url and sha256
  (builtins.fetchurl {
    url = "https://raw.githubusercontent.com/aaptel/notmuch-extract-patch/f732a53e12a7c91a06755ebfab2007adc9b3063b/notmuch-extract-patch";
    sha256 = "0nawkl04sj7psw6ikzay7kydj3dhd0fkwghcsf5rzaw4bmp4kbax";
  })

  # fetchurl with url and sha256 (as SRI)
  (builtins.fetchurl {
    url = "https://raw.githubusercontent.com/aaptel/notmuch-extract-patch/f732a53e12a7c91a06755ebfab2007adc9b3063b/notmuch-extract-patch";
    sha256 = "sha256-Xa1Jbl2Eq5+L0ww+Ph1osA3Z/Dxe/RkN1/dITQCdXFk=";
  })

  # fetchurl with another url, but same name
  (builtins.fetchurl {
    url = "https://test.example/owo";
    name = "notmuch-extract-patch";
    sha256 = "sha256-Xa1Jbl2Eq5+L0ww+Ph1osA3Z/Dxe/RkN1/dITQCdXFk=";
  })

  # The following tests use <nix/fetchurl.nix>.
  # This is a piece of Nix code producing a "fake derivation" which gets
  # handled by a "custom builder" that does the actual fetching.
  # We access `.outPath` here, as the current string output of a Derivation
  # still differs from the way nix presents it.
  # It behaves similar to builtins.fetchurl, except it requires a hash to be
  # provided upfront.
  # If `unpack` is set to true, it can unpack NAR files (decompressing if
  # necessary)
  # If `executable` is set to true, it will place the fetched file at the root,
  # but make it executable, and the hash is on the NAR representation.

  # Fetch a URL.
  (import <nix/fetchurl.nix> {
    url = "https://test.example/owo";
    name = "notmuch-extract-patch";
    sha256 = "Xa1Jbl2Eq5+L0ww+Ph1osA3Z/Dxe/RkN1/dITQCdXFk=";
  }).outPath

  # Fetch a NAR and unpack it, specifying the sha256 of its NAR representation.
  (import <nix/fetchurl.nix> {
    url = "https://cache.nixos.org/nar/0r8nqa1klm5v17ifc6z96m9wywxkjvgbnqq9pmy0sgqj53wj3n12.nar.xz";
    sha256 = "sha256-oj6yfWKbcEerK8D9GdPJtIAOveNcsH1ztGeSARGypRA=";
    unpack = true;
  }).outPath

  # Fetch a NAR and unpack it, specifying its *sha1* of its NAR representation.
  (import <nix/fetchurl.nix> {
    url = "https://cache.nixos.org/nar/0r8nqa1klm5v17ifc6z96m9wywxkjvgbnqq9pmy0sgqj53wj3n12.nar.xz";
    hash = "sha1-F/fMsgwkXF8fPCg1v9zPZ4yOFIA=";
    unpack = true;
  }).outPath

  # Fetch a URL, specifying the *sha1* of a NAR describing it as executable at the root.
  (import <nix/fetchurl.nix> {
    url = "https://cache.nixos.org/nar/0r8nqa1klm5v17ifc6z96m9wywxkjvgbnqq9pmy0sgqj53wj3n12.nar.xz";
    hash = "sha1-NKNeU1csW5YJ4lCeWH3Z/apppNU=";
    executable = true;
  }).outPath
]
