#!/usr/bin/env bash
# Build release artifacts for the committed HEAD on macOS.
#
# Usage: scripts/build-release.sh
#
# The build uses a clean export of HEAD, so untracked and private files stay out of
# it. Docker must be running. Output goes to target/dist/v<VERSION>/.
set -euo pipefail

fail() {
    echo "error: $*" >&2
    exit 1
}

[[ $(uname -s) == Darwin ]] || fail "this script builds macOS releases only"
root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
git -C "$root" diff --quiet HEAD || fail "tracked files have uncommitted changes"
docker info >/dev/null 2>&1 || fail "Docker must be running"

commit=$(git -C "$root" rev-parse HEAD)
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/host/Cargo.toml")
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] ||
    fail "cannot read the release version from host/Cargo.toml"
tag=v$version
if tagged=$(git -C "$root" rev-parse -q --verify "refs/tags/$tag^{commit}"); then
    [[ $tagged == "$commit" ]] || fail "tag $tag exists on another commit"
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/greenhold-release.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir "$work/src" "$work/stage"
git -C "$root" archive "$commit" | tar -x -C "$work/src"
src=$(cd "$work/src" && pwd -P)
cd "$src"

# Remove machine-specific paths from the binary. With the rust-src component
# installed, standard library paths point to the local toolchain.
unset CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR CARGO_BUILD_RUSTFLAGS CARGO_ENCODED_RUSTFLAGS \
    RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
cargo_home=$(cd "${CARGO_HOME:-$HOME/.cargo}" && pwd -P)
rust_src=$(rustc --print sysroot)/lib/rustlib/src/rust
rustc_commit=$(rustc -vV | sed -n 's/^commit-hash: //p')
unset RUSTFLAGS
sep=$'\x1f'
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$src=/greenhold$sep\
--remap-path-prefix=$cargo_home=/cargo$sep--remap-path-prefix=$rust_src=/rustc/$rustc_commit"

cargo reproducible
binary=$src/target/reproducible/greenhold
[[ $("$binary" --version) == "greenhold $version" ]] ||
    fail "binary version does not match $version"

target=$(rustc -vV | sed -n 's/^host: //p')
name=greenhold-$tag-$target
out=$root/target/dist/$tag
rm -rf "$out"
mkdir -p "$out"

# Fix archive metadata so the archive bytes depend only on the binary and commit.
cp "$binary" "$work/stage/greenhold"
chmod 755 "$work/stage/greenhold"
epoch=$(git -C "$root" show -s --format=%ct "$commit")
TZ=UTC touch -t "$(date -u -r "$epoch" +%Y%m%d%H%M.%S)" "$work/stage/greenhold"
COPYFILE_DISABLE=1 tar --format ustar --uid 0 --gid 0 --uname root --gname root \
    --no-xattrs --no-acls --no-fflags -cf - -C "$work/stage" greenhold |
    gzip -n -9 >"$out/$name.tar.gz"
(cd "$out" && shasum -a 256 "$name.tar.gz" >SHA256SUMS)

cat >"$out/$name.build-info.txt" <<EOF
version: $version
commit: $commit
target: $target
guest image ID: $(tr -d '[:space:]' <guest-builder/image-id)
greenhold SHA-256: $(shasum -a 256 "$binary" | cut -d ' ' -f 1)
archive SHA-256: $(cut -d ' ' -f 1 "$out/SHA256SUMS")
build command: scripts/build-release.sh
rustc: $(rustc -V)
cargo: $(cargo -V)
clang: $(clang --version | head -n 1)
linker: $(ld -v 2>&1 | head -n 1)
macOS: $(sw_vers -productVersion) ($(sw_vers -buildVersion))
macOS SDK: $(xcrun --show-sdk-version) ($(xcrun --show-sdk-build-version))
EOF

echo
echo "Release artifacts: $out"
cat "$out/$name.build-info.txt"
[[ -n ${tagged:-} ]] || echo "Tag $tag does not exist yet."
