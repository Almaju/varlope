A `pub` item inside a private module looks like API but nobody outside the crate can reach it, so
readers can't tell what consumers depend on and `dead_code` stops warning when it goes unused.
Reviewers keep asking "does this need to be `pub`?"; `pub(crate)` (or private) says the item is an
implementation detail, and rustc's `unreachable_pub` lint finds them all with a machine-applicable fix.
