[
  # This is independent of builtins
  (builtins.length [ (builtins.throw "Ferge") (builtins.throw "Wehsal") ])
  (builtins.attrNames {
    Hans = throw "Castorp";
    Joachim = throw "Ziemßen";
    James = "Tienappel";
  })

  (builtins.length (builtins.map builtins.throw [ "Settembrini" "Naphta" ]))

  (builtins.attrNames (builtins.mapAttrs builtins.throw {
    Clawdia = "Chauchat";
    Mynheer = "Peeperkorn";
  }))

  (builtins.length (builtins.genList (builtins.add "Marusja") 981))
  (builtins.length (builtins.genList builtins.throw 3))

  # These are hard to get wrong since the outer layer needs to be forced anyways
  (builtins.length (builtins.genericClosure {
    startSet = [
      { key = 1; initial = true; }
    ];
    operator = { key, initial, ... }:
      if initial
      then [ { key = key - 1; initial = false;  value = throw "lol"; } ]
      else [ ];
  }))
  (builtins.length (builtins.concatMap (m: [ m (builtins.throw m) ]) [ "Marusja" ]))
]
