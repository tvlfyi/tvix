let
  toStringableSet = {
    __toString = self: self.content;
    content = "Hello World";
  };

  toStringExamples = [
    null
    [ null false ]
    [ null /deep/thought ]
    [ [ null 2 ] null 3 ]
    [ false "flat" ]
    1
    4.2
    null
    false
    true
    "foo"
    /etc
    toStringableSet
    { __toString = _: toStringableSet; }
    { __toString = _: true; }
    { outPath = "out"; }
    { outPath = { outPath = { __toString = _: 2; }; }; }
  ];
in

(builtins.map toString toStringExamples) ++ [ (toString toStringExamples) ]
