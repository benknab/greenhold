#!/usr/bin/env bash
# Build macOS release artifacts from a clean export of the committed HEAD, so untracked and
# private files stay out. Docker must be running. Output goes to target/dist/v<VERSION>/.
set -euo pipefail
fail() { echo "error: $*" >&2 && exit 1; }

[[ $(uname -s) == Darwin ]] || fail "this script builds macOS releases only"
root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
cd "$root"
git diff --quiet HEAD || fail "tracked files have uncommitted changes"
docker info >/dev/null 2>&1 || fail "Docker must be running"
read -r commit epoch < <(git log -1 --no-show-signature --format='%H %ct')
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' host/Cargo.toml)
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] ||
    fail "cannot read the release version from host/Cargo.toml"
tagged=$(git rev-parse -q --verify "refs/tags/v$version^{commit}" || true)
[[ -z $tagged || $tagged == "$commit" ]] || fail "tag v$version exists on another commit"

work=$(mktemp -d "${TMPDIR:-/tmp}/greenhold-release.XXXXXX")
trap 'rm -rf "$work"' EXIT
cd -P "$work"
git -C "$root" archive "$commit" | tar -x

# Remove machine-specific paths from the binary; Cargo sees the physical path from `cd -P`.
# With rust-src installed, standard library paths point to the local toolchain. Linker
# option -S drops object paths; their lengths would change the Mach-O UUID.
unset CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
read -r rustc_commit target < <(rustc -vV | sed -En 's/^(commit-hash|host): //p' | xargs)
cargo_home=$(cd "${CARGO_HOME:-$HOME/.cargo}" && pwd -P)
sysroot=$(rustc --print sysroot)
printf -v flags '\x1f--remap-path-prefix=%s' "$PWD=/greenhold" "$cargo_home=/cargo" \
    "$sysroot/lib/rustlib/src/rust=/rustc/$rustc_commit"
export CARGO_ENCODED_RUSTFLAGS=-Clink-arg=-Wl,-S$flags
cargo reproducible
binary=target/reproducible/greenhold
version_output=$("$binary" --version)
[[ $(head -n 1 <<<"$version_output") == "greenhold $version" ]] ||
    fail "binary version does not match $version"
image_id=$(sed -n 's/^guest image ID: \([0-9a-f]\{64\}\)$/\1/p' <<<"$version_output")
[[ -n $image_id ]] || fail "binary does not report a guest image ID"
name=greenhold-v$version-$target
out=$root/target/dist/v$version
rm -rf "$out" && mkdir -p "$out" || exit

# Fix archive metadata so the archive bytes depend only on the binary, license, and commit.
chmod 755 "$binary"
chmod 644 LICENSE
TZ=UTC touch -t "$(date -u -r "$epoch" +%Y%m%d%H%M.%S)" "$binary" LICENSE
COPYFILE_DISABLE=1 tar --format ustar --uid 0 --gid 0 --uname root --gname root --no-xattrs \
    --no-acls --no-fflags -cf - LICENSE -C target/reproducible greenhold | gzip -n -9 >"$out/$name.tar.gz"
(cd "$out" && shasum -a 256 "$name.tar.gz" >SHA256SUMS)
printf '\nRelease artifacts: %s\n' "$out"
tee "$out/$name.build-info.txt" <<EOF
version: $version
commit: $commit
target: $target
guest image ID: $image_id
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
[[ -n $tagged ]] || echo "Tag v$version does not exist yet."
