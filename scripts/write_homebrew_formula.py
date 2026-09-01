#!/usr/bin/env python3
"""Write the LogCrab Homebrew formula for a published release."""

from __future__ import annotations

import argparse
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description="Write the LogCrab Homebrew formula.")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--arm-sha256", required=True)
    parser.add_argument("--intel-sha256", required=True)
    args = parser.parse_args()

    release_url = f"https://github.com/daniel-freiermuth/logcrab/releases/download/v{args.version}"
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        f'''class Logcrab < Formula
  desc "High-performance log file viewer"
  homepage "https://github.com/daniel-freiermuth/logcrab"
  version "{args.version}"

  on_arm do
    url "{release_url}/logcrab_aarch64_macos.tar.gz"
    sha256 "{args.arm_sha256}"
  end

  on_intel do
    url "{release_url}/logcrab_x86_64_macos.tar.gz"
    sha256 "{args.intel_sha256}"
  end

  def install
    bin.install "logcrab"
  end
end
'''
    )


if __name__ == "__main__":
    main()
