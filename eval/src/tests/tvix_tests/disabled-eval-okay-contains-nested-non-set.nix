# ? operator should work even if encountering a non-set value on the
# walk
{ a.b = 42; } ? a.b.c
