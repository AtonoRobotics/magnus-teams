#!/usr/bin/env bash
# Install local speech to text for voice input: whisper.cpp's whisper-cli and
# a model, so voice never leaves the appliance. Run as the appliance's user:
#
#     bash config/install-whisper.sh [model]
#
# The whisper.cpp version is the newest release tag, read from the repository
# at install time. The model (default "base", about 140 MB, multilingual) is
# fetched with whisper.cpp's own download script. cmake comes from a private
# Python venv, so nothing is installed system-wide. Prints the lines to add to
# magnus.toml.
set -euo pipefail
model="${1:-base}"
dest="${MAGNUS_WHISPER_DIR:-$HOME/.magnus-appliance/whisper}"
mkdir -p "$dest"
tag=$(git ls-remote --tags --sort=-v:refname https://github.com/ggml-org/whisper.cpp 'v*' | grep -v '\^{}' | head -1 | sed 's|.*refs/tags/||')
echo "whisper.cpp $tag, model $model, into $dest"

python3 -m venv "$dest/venv"
"$dest/venv/bin/pip" install -q cmake
rm -rf "$dest/src"
git clone -q --depth 1 --branch "$tag" https://github.com/ggml-org/whisper.cpp "$dest/src"
cd "$dest/src"
"$dest/venv/bin/cmake" -B build -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DWHISPER_BUILD_TESTS=OFF >/dev/null
"$dest/venv/bin/cmake" --build build -j"$(nproc)" --target whisper-cli >/dev/null
cp build/bin/whisper-cli "$dest/whisper-cli"
sh models/download-ggml-model.sh "$model" >/dev/null
mv "models/ggml-$model.bin" "$dest/ggml-$model.bin"
echo "$tag" > "$dest/VERSION"

cat <<EOF

Installed. Add to magnus.toml, then restart the appliance:

[speech]
whisper = "$dest/whisper-cli"
model = "$dest/ggml-$model.bin"
EOF
