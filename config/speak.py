"""Read-aloud for the appliance: text on stdin, MP3 on stdout, with Kokoro.

    speak --model kokoro.onnx --voices voices.bin --voice af_heart

`config/install-voice.sh` installs this as `speak` next to its own Python.
The voice's first letter is its language, as Kokoro names them.
"""
import argparse
import io
import sys

import soundfile
from kokoro_onnx import Kokoro

LANGUAGES = {"a": "en-us", "b": "en-gb", "e": "es", "f": "fr-fr", "h": "hi", "i": "it", "j": "ja", "p": "pt-br", "z": "cmn"}

p = argparse.ArgumentParser()
p.add_argument("--model", required=True)
p.add_argument("--voices", required=True)
p.add_argument("--voice", required=True)
a = p.parse_args()

text = sys.stdin.read().strip()
if not text:
    sys.exit("nothing to read")
samples, rate = Kokoro(a.model, a.voices).create(text, voice=a.voice, lang=LANGUAGES.get(a.voice[0], "en-us"))
out = io.BytesIO()
soundfile.write(out, samples, rate, format="MP3")
sys.stdout.buffer.write(out.getvalue())
