[
  # (fetchurl "url") cannot be tested, as that one has to fetch from the
  # internet to calculate the path.

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
]
