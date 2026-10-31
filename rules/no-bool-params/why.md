A call like `set_participation(true, false)` or `get_access_token(scope, None, false)` can't be read
without opening the definition, and swapping two flags still compiles. Reviewers repeatedly asked
for an enum (`AmpSuggestionType::Mobile` instead of `is_amp_mobile: bool`) or a struct with named
fields, which also stops invalid combinations from being representable.
