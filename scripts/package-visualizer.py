"""Package only the separately built Rust viewer; never touch Go release archives."""
import argparse
import hashlib
import platform
from pathlib import Path
import re
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, default=Path("dist/visualizer"))
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", args.version):
        parser.error("version must be a portable filename component")
    system = {"Windows": "windows", "Darwin": "macos", "Linux": "linux"}.get(platform.system())
    arch = {"amd64": "amd64", "x86_64": "amd64", "arm64": "arm64", "aarch64": "arm64"}.get(platform.machine().lower())
    if system is None or arch is None:
        parser.error("unsupported native release target")
    binary_name = "herdr-mesh-visualizer" + (".exe" if system == "windows" else "")
    if not args.binary.is_file() or args.binary.name != binary_name:
        parser.error(f"expected built native {binary_name}")
    args.output.mkdir(parents=True, exist_ok=True)
    archive = args.output / f"herdr-mesh-visualizer-{args.version}-{system}-{arch}.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as package:
        package.write(args.binary, binary_name)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    checksum = archive.with_suffix(".zip.sha256")
    checksum.write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
    print(archive)


if __name__ == "__main__":
    main()
