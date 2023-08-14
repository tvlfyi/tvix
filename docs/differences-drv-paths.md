---
title: ".drvPath inconsistencies"
author:
 - tazjin
 - flokli
email:
 - tazjin@tvl.su
 - flokli@flokli.de
lang: en-GB
---

# Why .drvPath differs between Nix and Tvix

Nix and Tvix currently use a different approach when it comes to tracking input
references, in order to build the right dependencies in advance.
Nix is using string contexts, whereas Tvix is doing reference scanning [^inbox-drvpath].

There are some real-life cases, for example during nixpkgs bootstrapping, where
multiple different fixed-output derivations are written to produce the same
hash.

For example, bootstrap sources that are downloaded early are fetched using
a special "builder hack", in which the `builder` field of the derivation is
populated with the magic string `builtins:fetchurl` and the builder itself will
perform a fetch, with everything looking like a normal derivation to the user.

These bootstrap sources are later on defined *again*, once `curl`is available,
to be downloaded using the standard pkgs.fetchtarball mechanism, but yielding
the *same* outputs (as the same files are being fetched).

In our reference scanning implementation, this output scanning of FOD will
cause the path of the *first* derivation producing the given fixed output to be
stored in the `inputDrvs` field of the derivation, while Nix will point to the
derivation that was actually used.

This doesn't cause any differences in the calculated *output paths*, as paths to
fixed-output derivations are replaced with a special
`fixed:out:${algo}:${digest}:${fodPath}` string that doesn't contain the "path
to the wrong derivation" anymore.

As we haven't fully determined if our reference scanning approach is gonna work,
and comparing output paths is sufficient to determine equality of the build
instructions, this is left as a future work item.


[^inbox-drvpath]: https://inbox.tvl.su/depot/20230316120039.j4fkp3puzrtbjcpi@tp/T/#t
