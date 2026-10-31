A UniFFI item declared both in the `.udl` and with proc-macros, or a `typedef` whose kind doesn't
match the real declaration, breaks the bindings: when encryption moved to db-crypto, `logins.udl`
kept `create_key` and used `interface` for a `trait_with_foreign`, producing duplicate Swift
definitions that broke the iOS build. New code uses proc-macros, so the UDL copy should go when an
item moves.
