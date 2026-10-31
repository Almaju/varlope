Kotlin and Swift callers can only usefully catch the errors they handle differently; everything else
should land in one catch-all that carries a `reason` and is sent to crash reporting with
`report_error`. `FxaError::Other` once carried no message, so apps only saw "other error" (#6278),
and reviewers keep asking "how do we expect a consumer to recover from this?" about JSON, SQL and
never-thrown variants.
