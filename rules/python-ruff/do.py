import sys


def main():
    args = parse_args()
    ok = upload_symbols(args.zip_file, args.token_file)
    return 0 if ok else 2


if __name__ == "__main__":
    sys.exit(main())
