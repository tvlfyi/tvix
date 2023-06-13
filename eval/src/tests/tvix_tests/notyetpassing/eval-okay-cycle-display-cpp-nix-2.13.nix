let
  linkedList = {
    car = 42;
    cdr = linkedList;
  };

  list = [
    linkedList
    linkedList
    linkedList
  ];

  set = {
    val = 42;
    wal = set;
    xal = set;
  };

  multiTail = {
   val = 42;
   tail1 = multiTail;
   tail2 = multiTail;
  };
in

[
  linkedList
  list
  set

  # In C++ Nix 2.3 these would be displayed differently
  multiTail
  (let multiTail = { val = 21; tail1 = multiTail; tail2 = multiTail; }; in multiTail)
]
