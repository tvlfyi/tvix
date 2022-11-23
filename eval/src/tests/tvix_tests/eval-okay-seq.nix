(builtins.seq 1 2) + (builtins.seq [ (throw "list") ] 20) + (builtins.seq { boing = throw "set"; } 20)
