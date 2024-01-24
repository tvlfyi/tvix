{
  attrspat = args@{ x, y, z }: x;
  attrspat-ellipsis = args@{ x, y, z, ... }: x;

  noattrspat = { x, y, z }: x;
  noattrspat-ellipsis = { x, y, z, ... }: x;
}
