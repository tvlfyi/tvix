# attrset-opcodes

The problem with attrset literals is twofold:

1. The keys of attribute sets may be dynamically evaluated.

   Access:

   ```nix
   let
     k = "foo";
     attrs = { /* etc. */ };
   in attrs."${k}"
   ```

   Literal:
   ```nix
   let
     k = "foo";
   in {
     "${k}" = 42;
   }
   ```

   The problem with this is that the attribute set key is not known at
   compile time, and needs to be dynamically evaluated by the VM as an
   expression.

   For the most part this should be pretty simple, assuming a
   theoretical instruction set:

   ```
   0000  OP_CONSTANT(0) # key "foo"
   0001  OP_CONSTANT(1) # value 42
   0002  OP_ATTR_SET(1) # construct attrset from 2 stack values
   ```

   The operation pushing the key needs to be replaced with one that
   leaves a single value (the key) on the stack, i.e. the code for the
   expression, e.g.:

   ```
   0000..000n <operations leaving a string value on the stack>
   000n+1     OP_CONSTANT(1) # value 42
   000n+2     OP_ATTR_SET(1) # construct attrset from 2 stack values
   ```

   This is fairly easy to do by simply recursing in the compiler when
   the key expression is encountered.

2. The keys of attribute sets may be nested.

   This is the non-trivial part of dealing with attribute set
   literals. Specifically, the nesting can be arbitrarily deep and the
   AST does not guarantee that related set keys are located
   adjacently.

   Furthermore, this frequently occurs in practice in Nix. We need a
   bytecode representation that makes it possible to construct nested
   attribute sets at runtime.

   Proposal: AttrPath values

   If we can leave a value representing an attribute path on the
   stack, we can offload the construction of nested attribute sets to
   the `OpAttrSet` operation.

   Under the hood, OpAttrSet in practice constructs a `Map<NixString,
   Value>` attribute set in most cases. This means it expects to pop
   the value of the key of the stack, but is otherwise free to do
   whatever it wants with the underlying map.

   In a simple example, we could have code like this:

   ```nix
   {
     a.b = 15;
   }
   ```

   This would be compiled to a new `OpAttrPath` instruction that
   constructs and pushes an attribute path from a given number of
   fragments (which are popped off the stack).

   For example,

   ```
   0000 OP_CONSTANT(0)  # key "a"
   0001 OP_CONSTANT(1)  # key "b"
   0002 OP_ATTR_PATH(2) # construct attrpath from 2 fragments
   0003 OP_CONSTANT(2)  # value 42
   0004 OP_ATTRS(1)     # construct attrset from one pair
   ```

   Right before `0004` the stack would be left like this:

   [ AttrPath[a,b], 42 ]

   Inside of the `OP_ATTRS` instruction we could then begin
   construction of the map and insert the nested attribute sets as
   required, as well as validate that there are no duplicate keys.

3. Both of these cases can occur simultaneously, but this is not a
   problem as the opcodes combine perfectly fine, e.g.:

   ```nix
   let
     k = "a";
   in {
     "${k}".b = 42;
   }
   ```

   results in

   ```
   0000..000n <operations leaving a string value on the stack>
   000n+1     OP_CONSTANT(1)  # key "b"
   000n+2     OP_ATTR_PATH(2) # construct attrpath from 2 fragments
   000n+3     OP_CONSTANT(2)  # value 42
   000n+4     OP_ATTR_SET(1)  # construct attrset from 2 stack values
   ```
