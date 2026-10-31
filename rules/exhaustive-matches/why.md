ads-client's `impl Hash for AdRequest` omitted query params, the endpoint and the OHTTP flag, so different requests could collide in the HTTP cache.
