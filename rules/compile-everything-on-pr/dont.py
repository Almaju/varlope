def run_rust_checks(changed_packages, features):
    cmdline = ["cargo", "clippy", "--all-targets"]
    for p in changed_packages:  # flagged: dependents of changed crates never build
        cmdline.extend(["-p", p.name])
    cmdline.extend(features.cmdline_args())
    run_command(cmdline)
