import sys


def main():
    args = parse_args()
    if not upload_symbols(args.zip_file, args.token_file):
        print("Symbol upload failed", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
