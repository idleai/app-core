#!/usr/bin/env python3
"""Check a released consumer with the candidate app-core in a temporary directory."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import urllib.request


def download(specification, destination):
    package = specification["package"]
    version = specification["tag"].removeprefix(f"{package}-v")
    filename = f"{package}-{version}.crate"
    archive = destination / filename
    checksum = destination / (filename + ".sha256")
    for path in [archive, checksum]:
        mirror = next((Path(directory) / path.name for directory in
                       os.environ.get("IDLE_ARTIFACTS_MIRROR", "").split(os.pathsep)
                       if directory and (Path(directory) / path.name).is_file()), None)
        if mirror:
            shutil.copyfile(mirror, path)
        else:
            url = f"https://github.com/{specification['repository']}/releases/download/{specification['tag']}/{path.name}"
            with urllib.request.urlopen(url, timeout=120) as response, path.open("wb") as output:
                shutil.copyfileobj(response, output)
    if (hashlib.sha256(archive.read_bytes()).hexdigest() != specification["sha256"]
            or specification["sha256"] != checksum.read_text().split()[0]):
        raise ValueError(f"consumer archive checksum mismatch: {filename}")
    with tarfile.open(archive) as bundle:
        bundle.extractall(destination, filter="data")
    return destination / f"{package}-{version}"


def main():
    root = Path(__file__).resolve().parent.parent
    releases = json.loads((root / "consumer-dependencies.json").read_text())["dependencies"]
    for consumer in sys.argv[1:] or releases:
        with tempfile.TemporaryDirectory(prefix="idle-consumer-") as temporary:
            staging = Path(temporary)
            if consumer in releases:
                source = download(releases[consumer], staging)
                package = releases[consumer]["package"]
            else:
                source = staging / "source"
                checkout = Path(consumer).resolve()
                if not (checkout / "Cargo.toml").is_file():
                    raise ValueError("provide a released consumer name or an explicit local consumer directory")
                shutil.copytree(checkout, source, ignore=shutil.ignore_patterns(
                    ".git", "target", "node_modules", "dist", "out", "bin", ".artifacts", "releases"))
                metadata = json.loads(subprocess.check_output(
                    ["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=source))
                package = metadata["packages"][0]["name"]
            config = staging / "candidate.toml"
            config.write_text((root / '.cargo/config.toml').read_text() + '\n[patch."sparse+https://raw.githubusercontent.com/idleai/app-core/cargo-index/"]\napp-core = { path = '
                              + json.dumps(str(root / "crates/app-core")) + ' }\n')
            cargo = ["cargo", "--config", str(config)]
            commands = [
                ["generate-lockfile"],
                ["check", "--workspace", "--all-targets", "--all-features", "--locked"],
                ["test", "--locked", "-p", package, "--lib"],
                ["check", "--locked", "-p", package, "--lib", "--all-features",
                 "--target", "wasm32-unknown-unknown"],
            ]
            toolchain = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
            environment = dict(os.environ, CARGO_TARGET_DIR=str(root / "target/consumers"),
                               RUSTUP_TOOLCHAIN=os.environ.get("RUSTUP_TOOLCHAIN", toolchain))
            for arguments in commands:
                subprocess.run(cargo + arguments, cwd=source, env=environment, check=True)


if __name__ == "__main__":
    main()
