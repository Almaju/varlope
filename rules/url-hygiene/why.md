Gluing URLs together with `format!` skips encoding and gets slashes wrong: the MARS base URLs had no
trailing `/`, so `Url::join` silently dropped `/v1` (#7046), and a `format!("{}/...")` produced a
double slash and a 404 while a relative `Location` header was fed to `Url::parse` (#7543). Parse once
into a `url::Url` with a base ending in `/`, then use `join`, `path_segments_mut()` and
`query_pairs_mut()`, and keep each service endpoint in a single `const`.
