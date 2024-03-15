# the 'from' part of an `inherit` can be any expression.
{ inherit ({ a = 15; }) a; }.a
