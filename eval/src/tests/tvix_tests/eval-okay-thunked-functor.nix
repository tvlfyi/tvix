let
  __functor = f;
  f = self: x: self.out * x;
in
{
  inherit __functor;
  out = 21;
} 2
