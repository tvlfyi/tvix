let
  attrs1 = { x = 1 + 2; };
  attrs2 = { x = 2 + 1; };
  list1 = [ (1 + 2) ];
  list2 = [ (2 + 1) ];
  list3 = [ (2 + 2) ];
in [
  (attrs1 == attrs2)
  (list1 == list2)
  (list3 == list2)
]
