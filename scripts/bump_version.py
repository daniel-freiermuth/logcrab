#!/usr/bin/env python3
"""
Version bump script for LogCrab.

Follows conventional commits:
- feat: commits bump minor version
- Other commits bump patch version
"""

import re
import subprocess
import sys


def run(cmd: list[str], check: bool = True, capture: bool = True) -> subprocess.CompletedProcess:
    """Run a command and return the result."""
    print(f"$ {' '.join(cmd)}")
    try:
        result = subprocess.run(cmd, capture_output=capture, text=True, check=check)
    except subprocess.CalledProcessError as e:
        print(f"\n--- command failed (exit {e.returncode}) ---", file=sys.stderr)
        if e.stdout:
            print("stdout:", file=sys.stderr)
            print(e.stdout.rstrip(), file=sys.stderr)
        if e.stderr:
            print("stderr:", file=sys.stderr)
            print(e.stderr.rstrip(), file=sys.stderr)
        print("---", file=sys.stderr)
        raise
    return result


def is_dirty() -> bool:
    """Check if the git working directory is dirty."""
    result = run(["git", "status", "--porcelain"])
    return bool(result.stdout.strip())


def get_latest_tag() -> str | None:
    """Get the latest git tag."""
    result = run(["git", "describe", "--tags", "--abbrev=0"], check=False)
    if result.returncode != 0:
        return None
    return result.stdout.strip()


def get_commits_since_tag(tag: str | None) -> list[str]:
    """Get commit messages since the given tag."""
    if tag:
        result = run(["git", "log", f"{tag}..HEAD", "--pretty=format:%s"])
    else:
        result = run(["git", "log", "--pretty=format:%s"])
    return result.stdout.strip().split("\n") if result.stdout.strip() else []


def has_feat_commit(commits: list[str]) -> bool:
    """Check if any commit is a feat: commit."""
    feat_pattern = re.compile(r"^feat(\(.+\))?!?:")
    return any(feat_pattern.match(commit) for commit in commits)


def parse_version(version: str) -> tuple[int, int, int]:
    """Parse a semver version string."""
    match = re.match(r"(\d+)\.(\d+)\.(\d+)", version)
    if not match:
        raise ValueError(f"Invalid version: {version}")
    return int(match.group(1)), int(match.group(2)), int(match.group(3))


def bump_version(version: str, is_minor: bool) -> str:
    """Bump the version string."""
    major, minor, patch = parse_version(version)
    if is_minor:
        return f"{major}.{minor + 1}.0"
    else:
        return f"{major}.{minor}.{patch + 1}"


def get_current_version() -> str:
    """Get the current version from Cargo.toml."""
    with open("Cargo.toml", "r") as f:
        content = f.read()
    match = re.search(r'^version\s*=\s*"([^"]+)"', content, re.MULTILINE)
    if not match:
        raise ValueError("Could not find version in Cargo.toml")
    return match.group(1)


def update_cargo_toml(new_version: str) -> None:
    """Update the version in Cargo.toml."""
    with open("Cargo.toml", "r") as f:
        content = f.read()
    content = re.sub(
        r'^(version\s*=\s*)"[^"]+"',
        f'\\1"{new_version}"',
        content,
        count=1,
        flags=re.MULTILINE
    )
    with open("Cargo.toml", "w") as f:
        f.write(content)
    print(f"Updated Cargo.toml to version {new_version}")


def update_pkgbuild(new_version: str) -> None:
    """Update the version in PKGBUILD."""
    with open("PKGBUILD", "r") as f:
        content = f.read()
    content = re.sub(
        r'^(pkgver=).+$',
        f'\\g<1>{new_version}',
        content,
        count=1,
        flags=re.MULTILINE
    )
    with open("PKGBUILD", "w") as f:
        f.write(content)
    print(f"Updated PKGBUILD to version {new_version}")


def cargo_check() -> bool:
    """Run cargo check."""
    print("\nRunning cargo check...")
    result = run(["cargo", "check"], capture=False, check=False)
    return result.returncode == 0


def commit_changes(new_version: str) -> None:
    """Commit the version bump changes."""
    run(["git", "add", "Cargo.toml", "Cargo.lock", "PKGBUILD"])
    run(["git", "commit", "-m", f"chore: bump version to {new_version}"])
    print(f"Committed version bump to {new_version}")


def create_tag(new_version: str) -> None:
    """Create a git tag for the new version."""
    tag = f"v{new_version}"
    run(["git", "tag", "-a", tag, "-m", f"Release {new_version}"])
    print(f"Created tag {tag}")


def git_push() -> None:
    """Push commits and tags to remote."""
    run(["git", "push"], capture=False)
    run(["git", "push", "--tags"], capture=False)
    print("Pushed commits and tags to remote")


def cargo_deb() -> str:
    """Build the Debian package and rename it."""
    import glob
    
    print("\nBuilding Debian package...")
    run(["cargo", "deb"], capture=False)
    
    # Find and rename the generated .deb file
    deb_files = glob.glob("target/debian/*.deb")
    if deb_files:
        original = deb_files[0]
        new_name = "target/debian/logcrab_amd64_ubuntu24.deb"
        run(["mv", original, new_name])
        print(f"Renamed {original} to {new_name}")
        return new_name
    return ""


def main() -> int:
    # Step 1: Check if directory is dirty
    print("Checking git status...")
    if is_dirty():
        print("Error: Working directory is dirty. Please commit or stash changes first.")
        return 1

    # Step 2: Determine version bump type
    current_version = get_current_version()
    print(f"Current version: {current_version}")

    latest_tag = get_latest_tag()
    print(f"Latest tag: {latest_tag or '(none)'}")

    commits = get_commits_since_tag(latest_tag)
    if not commits:
        print("No commits since last tag. Nothing to do.")
        return 0

    print(f"\nCommits since {latest_tag or 'beginning'}:")
    for commit in commits:
        print(f"  - {commit}")

    is_minor = has_feat_commit(commits)
    bump_type = "minor" if is_minor else "patch"
    new_version = bump_version(current_version, is_minor)

    print(f"\nBump type: {bump_type}")
    print(f"New version: {new_version}")

    # Confirm with user
    response = input("\nProceed with version bump? [y/N] ").strip().lower()
    if response != "y":
        print("Aborted.")
        return 0

    # Step 2: Update version files
    update_cargo_toml(new_version)
    update_pkgbuild(new_version)

    # Step 3: Cargo check
    if not cargo_check():
        print("Error: cargo check failed. Reverting changes...")
        run(["git", "checkout", "Cargo.toml", "PKGBUILD"])
        return 1

    # Step 4: Commit changes
    commit_changes(new_version)

    # Step 5: Create tag
    create_tag(new_version)

    # Step 6: Push
    git_push()

    # Step 7: Build deb package
    cargo_deb()

    print(f"\n✓ Successfully released version {new_version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
