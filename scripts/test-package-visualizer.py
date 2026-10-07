"""Regression tests for viewer and Windows-only screensaver packaging."""
import contextlib
import hashlib
import importlib.util
import io
from pathlib import Path
import sys
import struct
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location(
    "package_visualizer", Path(__file__).with_name("package-visualizer.py")
)
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)


class PackagingTests(unittest.TestCase):
    def package(self, system, screensaver=False, machine="arm64", pe_machine=0xAA64):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            name = "herdr-mesh-screensaver.exe" if screensaver else (
                "herdr-mesh-visualizer" + (".exe" if system == "Windows" else "")
            )
            binary = root / name
            contents = bytearray(b"test executable contents")
            if system == "Windows":
                contents = bytearray(134)
                contents[:2] = b"MZ"
                struct.pack_into("<I", contents, 60, 128)
                contents[128:132] = b"PE\x00\x00"
                struct.pack_into("<H", contents, 132, pe_machine)
            binary.write_bytes(contents)
            output = root / "output"
            args = ["package-visualizer", "--binary", str(binary),
                    "--version", "test-version", "--output", str(output)]
            if screensaver:
                args.append("--screensaver")
            with patch.object(sys, "argv", args), \
                    patch.object(packager.platform, "system", return_value=system), \
                    patch.object(packager.platform, "machine", return_value=machine), \
                    contextlib.redirect_stdout(io.StringIO()):
                packager.main()
            archives = list(output.glob("*.zip"))
            self.assertEqual(len(archives), 1)
            archive = archives[0]
            with zipfile.ZipFile(archive) as package:
                entry = "herdr-mesh-visualizer.scr" if screensaver else name
                self.assertEqual(package.namelist(), [entry, "THIRD_PARTY_NOTICES.md"])
                self.assertIn(b"Copyright (c) 2026 Meng To", package.read("THIRD_PARTY_NOTICES.md"))
                self.assertIn(b"Permission is hereby granted", package.read("THIRD_PARTY_NOTICES.md"))
                self.assertEqual(package.read(entry), binary.read_bytes())
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            self.assertEqual(
                archive.with_suffix(".zip.sha256").read_text(encoding="utf-8"),
                f"{digest}  {archive.name}\n",
            )
            return archive.name

    def test_existing_three_platform_viewer_archive_names_are_preserved(self):
        for system, label in [("Windows", "windows"), ("Linux", "linux"), ("Darwin", "macos")]:
            with self.subTest(system=system):
                self.assertEqual(self.package(system),
                                 f"herdr-mesh-visualizer-test-version-{label}-arm64.zip")

    def test_separate_windows_screensaver_archive(self):
        self.assertEqual(self.package("Windows", True),
                         "herdr-mesh-screensaver-test-version-windows-arm64.zip")

    def test_emulated_python_does_not_mislabel_arm64_executables(self):
        self.assertEqual(self.package("Windows", True, machine="AMD64"),
                         "herdr-mesh-screensaver-test-version-windows-arm64.zip")
        self.assertEqual(self.package("Windows", machine="ARM64", pe_machine=0x8664),
                         "herdr-mesh-visualizer-test-version-windows-amd64.zip")

    def test_unsupported_pe_architecture_is_rejected(self):
        with self.assertRaises(SystemExit) as error, contextlib.redirect_stderr(io.StringIO()):
            self.package("Windows", True, pe_machine=0x14C)
        self.assertEqual(error.exception.code, 2)

    def test_other_platforms_reject_screensaver_packaging(self):
        for system in ["Linux", "Darwin"]:
            with self.subTest(system=system), self.assertRaises(SystemExit) as error, \
                    contextlib.redirect_stderr(io.StringIO()):
                self.package(system, True)
            self.assertEqual(error.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
