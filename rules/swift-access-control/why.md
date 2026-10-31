Swift declarations default to `internal`, which a consuming app such as firefox-ios cannot see, and
nothing fails until the app tries to call the API: hard-coded Nimbus features once shipped without
`public` and needed a follow-up release. Saying `public` (or an explicit `internal`/`private`) on
every declaration of the nimbus-fml Swift templates and any wrapper under `components/*/ios` makes
the intent visible in review.
