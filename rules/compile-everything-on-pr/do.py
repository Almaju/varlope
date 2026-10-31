def run_rust_checks(changed_packages, features):
    # Build every crate and target: a change to `logins` must also compile
    # `examples/sync-pass` and the integration-test crates.
    run_command(["cargo", "check", "--workspace", "--all-targets", "--locked"])
    cmdline = ["cargo", "clippy", "--all-targets"]
    for p in changed_packages:
        cmdline.extend(["-p", p.name])
    cmdline.extend(features.cmdline_args())
    run_command(cmdline)
