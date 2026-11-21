# Prints the next vX.Y.Z tag: reuse the tag already on HEAD, else Cargo.toml version if it
# is ahead of the newest tag (manual minor/major bump), else newest tag with patch + 1.
set -euo pipefail
current=$(git tag --points-at HEAD -l 'v*' | sort -V | tail -1)
if [ -n "$current" ]; then echo "$current"; exit 0; fi
cargo_v=$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)
latest=$(git tag -l 'v[0-9]*' | sed 's/^v//' | sort -V | tail -1)
if [ -z "$latest" ] || [ "$(printf '%s\n%s\n' "$latest" "$cargo_v" | sort -V | tail -1)" != "$latest" ]; then
  echo "v$cargo_v"
else
  IFS=. read -r major minor patch <<<"$latest"
  echo "v$major.$minor.$((patch + 1))"
fi
