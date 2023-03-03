# Application of unary operators on deferred formals arguments (via
# defaulting), see also b/255.
[
  (({ b ? !a, a }: b) { a = true; })
  (({ b ? -a, a }: b) { a = 2; })
]
