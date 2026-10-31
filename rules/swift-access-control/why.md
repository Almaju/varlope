Swift declarations default to `internal`, which a consuming app such as firefox-ios cannot see, and
nothing fails until the app tries to call the API. Hard-coded Nimbus features once shipped without
`public` and needed a follow-up release; saying `public` (or an explicit `internal`/`private`) on
every declaration makes the intent visible in review. In this repository that means the nimbus-fml
Swift templates and any hand-written wrapper under `components/*/ios`.
