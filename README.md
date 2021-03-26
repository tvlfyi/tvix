Tvix
====

For more information about Tvix, contact one of the project owners. We
are interested in people who would like to help us review designs,
brainstorm and describe requirements that we may not yet have
considered.

## License structure

All code implemented for Tvix is licensed under the GPL-3.0, with the
exception of the protocol buffer definitions used for communication
between services which are available under a more permissive license
(MIT).

The idea behind this structure is that any direct usage of our code
(e.g. linking to it, embedding the evaluator, etc.) will fall under
the terms of the GPL3, but users are free to implement their own
components speaking these protocols under the terms of the MIT
license.
