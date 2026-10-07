"""Package only the separately built Rust viewer; never touch Go release archives."""
import argparse
import hashlib
import platform
from pathlib import Path
import re
import struct
import zipfile


def windows_architecture(binary):
    """Use the executable, not an emulated Python interpreter's architecture."""
    with binary.open("rb") as source:
        header = source.read(64)
        if len(header) != 64 or header[:2] != b"MZ":
            raise ValueError("expected a Windows PE executable")
        source.seek(struct.unpack_from("<I", header, 60)[0])
        pe = source.read(6)
    if len(pe) != 6 or pe[:4] != b"PE\x00\x00":
        raise ValueError("expected a Windows PE executable")
    architecture = {0x8664: "amd64", 0xAA64: "arm64"}.get(struct.unpack_from("<H", pe, 4)[0])
    if architecture is None:
        raise ValueError("Windows executable must be amd64 or arm64")
    return architecture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, default=Path("dist/visualizer"))
    parser.add_argument("--screensaver", action="store_true",
                        help="package a Windows console-free screensaver in its own archive")
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", args.version):
        parser.error("version must be a portable filename component")
    system = {"Windows": "windows", "Darwin": "macos", "Linux": "linux"}.get(platform.system())
    arch = {"amd64": "amd64", "x86_64": "amd64", "arm64": "arm64", "aarch64": "arm64"}.get(platform.machine().lower())
    if system is None or (system != "windows" and arch is None):
        parser.error("unsupported native release target")
    if args.screensaver and system != "windows":
        parser.error("screensaver packaging is Windows-only")
    binary_name = "herdr-mesh-visualizer" + (".exe" if system == "windows" else "")
    if args.screensaver:
        binary_name = "herdr-mesh-screensaver.exe"
    if not args.binary.is_file() or args.binary.name != binary_name:
        parser.error(f"expected built native {binary_name}")
    if system == "windows":
        try:
            arch = windows_architecture(args.binary)
        except (ValueError, OSError) as error:
            parser.error(str(error))
    args.output.mkdir(parents=True, exist_ok=True)
    product = "herdr-mesh-screensaver" if args.screensaver else "herdr-mesh-visualizer"
    entry = "herdr-mesh-visualizer.scr" if args.screensaver else binary_name
    archive = args.output / f"{product}-{args.version}-{system}-{arch}.zip"
    notices = Path(__file__).resolve().parents[1] / "visualizer" / "THIRD_PARTY_NOTICES.md"
    if not notices.is_file():
        parser.error("Orb third-party notices are required for distribution")
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as package:
        package.write(args.binary, entry)
        package.write(notices, "THIRD_PARTY_NOTICES.md")
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    checksum = archive.with_suffix(".zip.sha256")
    checksum.write_text(f"{digest}  {archive.name}\n", encoding="utf-8")
    print(archive)


if __name__ == "__main__":
    main()
