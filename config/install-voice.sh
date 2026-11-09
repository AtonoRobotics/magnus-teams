#!/usr/bin/env bash
# Install the appliance's read-aloud voice: Kokoro, run on the appliance, so
# every app and browser hears the same voice and the text never leaves it.
# Run as the appliance's user:
#
#     bash config/install-voice.sh [voice]
#
# kokoro-onnx is the newest release on PyPI, in a private Python venv. The
# model and voices are the newest Kokoro files in kokoro-onnx's newest
# model-files release, read at install time. The voice (default "af_heart")
# is one of Kokoro's; its first letter is its language. Speaks a test sentence,
# then prints the lines to add to magnus.toml.
set -euo pipefail
voice="${1:-af_heart}"
here="$(cd "$(dirname "$0")" && pwd)"
dest="${MAGNUS_VOICE_DIR:-$HOME/.magnus-appliance/voice}"
mkdir -p "$dest"
repo=thewh1teagle/kokoro-onnx
tag=$(git ls-remote --tags --sort=-v:refname "https://github.com/$repo" 'model-files-v*' | grep -v '\^{}' | head -1 | sed 's|.*refs/tags/||')
assets=$(python3 -c '
import json, re, sys, urllib.request
names = [a["name"] for a in json.load(urllib.request.urlopen(f"https://api.github.com/repos/{sys.argv[1]}/releases/tags/{sys.argv[2]}"))["assets"]]
def newest(pattern):
    found = [(tuple(map(int, m.group(1).split("."))), n) for n in names if (m := re.fullmatch(pattern, n))]
    return max(found)[1]
print(newest(r"kokoro-v([\d.]+)\.onnx"), newest(r"voices-v([\d.]+)\.bin"))
' "$repo" "$tag")
read -r model voices <<<"$assets"
echo "kokoro-onnx model files $tag: $model, $voices, voice $voice, into $dest"

python3 -m venv "$dest/venv"
"$dest/venv/bin/pip" install -q --upgrade kokoro-onnx soundfile
for f in "$model" "$voices"; do
  curl -fsSL -o "$dest/$f" "https://github.com/$repo/releases/download/$tag/$f"
done
{ echo "#!$dest/venv/bin/python"; cat "$here/speak.py"; } > "$dest/speak"
chmod +x "$dest/speak"
echo "$tag $("$dest/venv/bin/pip" show kokoro-onnx | sed -n 's/^Version: /kokoro-onnx /p')" > "$dest/VERSION"

echo "Testing the voice..."
echo "Magnus can read aloud." | "$dest/speak" --model "$dest/$model" --voices "$dest/$voices" --voice "$voice" > "$dest/test.mp3"
test -s "$dest/test.mp3"
rm "$dest/test.mp3"

cat <<EOF

Installed. Add to magnus.toml, then restart the appliance:

[read_aloud]
speak = "$dest/speak"
model = "$dest/$model"
voices = "$dest/$voices"
voice = "$voice"
EOF
