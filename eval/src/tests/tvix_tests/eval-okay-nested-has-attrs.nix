let
  set = {
    a.b.c = 123;
    foo = {
      bar = 23;
    };
    baz = 1;
  };

  tes = "random value";
in

[
  (set ? a)
  (set ? a.b)
  (set ? a.b.c)
  (set ? foo)
  (set ? foo.bar)
  (set.foo ? bar)
  (set ? baz)
  (set ? x)
  (set ? x.y.z)
  (tes ? bar)
  (tes ? x.y.z)
  (null ? null)
]
