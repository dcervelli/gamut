# Releasing

A release is a tag. `bin/release` makes it, `git push` publishes it, and
`bin/pkgbuild-sha` points the package at it afterwards. Nothing else is
built or uploaded: the `Release` workflow in
[`.github/workflows/release.yml`](../.github/workflows/release.yml) turns
the tag into a GitHub release with generated notes, and the point of that
release is its source tarball, which is what
[`packaging/PKGBUILD`](../packaging/PKGBUILD) names in `source=`. Omarchy
builds the package from that tarball in an Arch container and signs the
result itself, so no binary is attached — there is nothing a binary could be
for that the tarball is not.

## Before

**The changelog is written by hand, and first.** `bin/release` does not
touch [CHANGELOG.md](../CHANGELOG.md); it insists on a clean tree, so the
changelog has to be committed before it runs. Rename `## Unreleased` to
`## X.Y.Z - YYYY-MM-DD` and leave nothing under an `Unreleased` heading
that is going out in this version. Versions follow semantic versioning, as
the changelog's own preamble says, so the number is decided by what is
under that heading, and `bin/release` without an argument only ever bumps
the patch level: anything more is given explicitly.

**The tools.** `cargo`, `git` and `reuse` have to be on the path, and
`cargo-about` has to be installed:

```sh
cargo install cargo-about --locked --features cli
```

`bin/release` regenerates `THIRD-PARTY-NOTICES` from the lock it is about
to tag, and CI fails a tree whose notices were generated from a different
lock; [licensing](licensing.md) says why that file is committed rather than
produced during the package build.

## Cutting it

```sh
bin/release            # the next patch level after the last v* tag
bin/release 0.2.0      # or exactly this version
```

The script, in order:

1. Refuses a dirty tree, a version that is not three numbers, and a tag
   that already exists.
2. Writes the version into `Cargo.toml` and into the PKGBUILD's `pkgver`,
   and resets `pkgrel=1` — a new upstream version is always release 1 of
   the package.
3. `cargo build`, which is what refreshes `Cargo.lock`'s own record of the
   version, then rewrites `THIRD-PARTY-NOTICES` from that lock, stamped
   with the lock's checksum.
4. Runs the checks CI runs, less `cargo doc` and `cargo audit`:
   `cargo fmt --check`, `clippy` with warnings denied, `cargo test`,
   `reuse lint`. `cargo test` includes `cli.rs`'s packaging
   tests, so a desktop entry, completion or PKGBUILD that drifted from the
   binary stops the release here, before there is a tag to undo.
5. Checks that the binary it just built reports `gamut X.Y.Z` — the
   version a user sees comes from `Cargo.toml` at compile time, and this
   is the proof the bump reached it.
6. Commits `Cargo.toml`, `Cargo.lock`, the PKGBUILD and the notices as
   `Release vX.Y.Z`, and tags that commit `vX.Y.Z`.

If any step fails, the four files are put back and nothing is committed;
the tree is as it was, and the failure is fixed and committed like any
other change before trying again. If it got as far as the tag but the tag
should not go out, undo it locally — `git tag -d vX.Y.Z` and
`git reset --hard HEAD~1` — which is safe only because nothing has been
pushed.

## Publishing it

```sh
git push && git push origin vX.Y.Z
```

The workflow checks that the tag agrees with the `version` in `Cargo.toml`
and creates the release. The `--generate-notes` it uses lists the commits
since the previous tag; the changelog is the account meant for people,
which is why it is written before the tag rather than reconstructed from
this.

**A pushed tag is never moved.** Its tarball's checksum is about to be
recorded in the PKGBUILD, and a tarball GitHub regenerates from a moved tag
is a different file, so every PKGBUILD that recorded the first one fails
`makepkg`'s integrity check from then on. Something wrong in a published
release is fixed in the next patch version.

## Pointing the package at it

```sh
bin/pkgbuild-sha vX.Y.Z
git commit -am "Update PKGBUILD"
git push
```

This is a separate step, and a separate commit, for a reason that cannot
be scripted around: the checksum is of a tarball that contains the
PKGBUILD, so the PKGBUILD cannot carry its own tarball's checksum. The
tagged tree's PKGBUILD names the new version but still carries the
previous release's `sha256sums`; the commit after the tag is what makes
`cd packaging && makepkg -si`, the install the README gives, build the
version just released. The PKGBUILD names a released tarball rather than
the working tree on purpose, so `makepkg` at any commit installs the last
release, not whatever is checked out.

The script fetches `$url/archive/refs/tags/vX.Y.Z.tar.gz`, retrying because
GitHub can take a few seconds to serve a tarball for a tag it has just
been given; checks that it unpacks into `gamut-X.Y.Z/`, the directory the
PKGBUILD's functions `cd` into; and writes the checksum into the first
entry of `sha256sums=`, which is the tarball's — any entries after it
belong to other sources and are left alone.

## What guards it

The release itself is not tested until someone builds the package, so the
checks that matter run before the tag:

- `cli.rs`'s tests and the CI step beside them assert that the PKGBUILD's
  `pkgver` is `Cargo.toml`'s `version`, that it packages `PROGRAM`, and
  that it installs the desktop entry and the icon under `APP_ID`. A
  PKGBUILD kept in the tree can fall behind the program it packages
  without anyone noticing; this is what notices.
- CI compares the lock stamp in `THIRD-PARTY-NOTICES` with `Cargo.lock`,
  so a dependency added without the notices being regenerated is caught on
  the push, not by the next release.
- CI runs `desktop-file-validate` on the entry and `namcap` on the
  PKGBUILD, statically: neither needs the tarball to exist.
