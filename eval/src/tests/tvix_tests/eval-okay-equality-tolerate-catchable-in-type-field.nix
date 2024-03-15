(builtins.tryEval (builtins.elem { type = rec { x = throw "fred"; }.x; } [{ type = 3; }])).success
