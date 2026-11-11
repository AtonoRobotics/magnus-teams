"""Tests for the desktop's theme and window rule (spec rev 145, "The
desktop"). Run with `python3 -I sandbox/desktop/test_desktop.py`; they need
no display.
"""
import importlib.util
import os
import re
import unittest

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.dirname(os.path.dirname(here))


def load(name):
    spec = importlib.util.spec_from_file_location(name, os.path.join(here, f"{name}.py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


theme = load("make_theme")
arrange = load("arrange")


class WindowRule(unittest.TestCase):
    def test_one_window_is_maximized(self):
        acts, known = arrange.layout([], [10])
        self.assertEqual(acts, [(10, "maximize")])
        self.assertEqual(known, [10])

    def test_a_second_window_snaps_both_to_halves_older_on_the_left(self):
        acts, known = arrange.layout([10], [10, 11])
        self.assertEqual(acts, [(10, "left"), (11, "right")])
        self.assertEqual(arrange.layout(known, [10, 11])[0], [], "two windows were moved again with nothing new")

    def test_a_third_window_opens_maximized_and_leaves_the_others(self):
        acts, _ = arrange.layout([10, 11], [10, 11, 12])
        self.assertEqual(acts, [(12, "maximize")])

    def test_back_to_one_window_it_is_maximized_and_back_to_two_they_snap(self):
        self.assertEqual(arrange.layout([10, 11], [11])[0], [(11, "maximize")])
        self.assertEqual(arrange.layout([10, 11, 12], [10, 12])[0], [(10, "left"), (12, "right")])

    def test_halves_fill_the_work_area_under_the_top_bar(self):
        area, ext = (0, 34, 1280, 766), (1, 1, 38, 1)
        self.assertEqual(arrange.half("left", area, ext), (0, 34, 638, 727))
        self.assertEqual(arrange.half("right", area, ext), (640, 34, 638, 727))


def app_tokens():
    """The app's own light and dark colors, from desktop/ui/theme.js."""
    with open(os.path.join(root, "desktop/ui/theme.js")) as f:
        src = f.read()
    out = {}
    for mode in ("LIGHT", "DARK"):
        block = src[src.index(f"const {mode} = {{"):]
        block = block[: block.index("};")]
        out[mode.lower()] = {k.strip('"'): v for k, v in re.findall(r'("?[\w-]+"?):\s*"(#[0-9A-Fa-f]{6})"', block)}
    blue = src[src.index('{ id: "blue"'):]
    blue = blue[: blue.index("\n")]
    for mode in ("light", "dark"):
        part = blue[blue.index(f"{mode}: {{"):]
        part = part[: part.index("}")]
        out[mode].update({k.strip('"'): v for k, v in re.findall(r'("?[\w-]+"?):\s*"(#[0-9A-Fa-f]{6})"', part)})
    return out


class Colors(unittest.TestCase):
    def test_every_desktop_color_is_one_of_the_apps(self):
        tokens = app_tokens()
        for mode, p in theme.PALETTES.items():
            allowed = {v.upper() for v in tokens[mode].values()}
            for name, value in p.items():
                for v in value if isinstance(value, list) else [value]:
                    self.assertIn(v.upper(), allowed, f"{mode} {name} {v} is not one of the app's colors")

    def test_the_wallpaper_is_the_artboards(self):
        with open(os.path.join(root, "docs/design/team-and-computer/Wallpaper.dc.html")) as f:
            src = f.read()
        for mode, c in theme.WALLPAPER.items():
            for k, v in c.items():
                self.assertIn(f"{k}: '{v}'", src, f"{mode} wallpaper {k}")
            svg = theme.wallpaper_svg(mode)
            self.assertIn(f'fill="{c["bg"]}"', svg)

    def test_both_looks_have_a_whole_window_theme(self):
        for mode in ("light", "dark"):
            files = theme.xfwm4_files(mode)
            for part in ["top-left", "top-right", "left", "right", "bottom", "bottom-left", "bottom-right"] + [f"title-{i}" for i in range(1, 6)]:
                for state in ("active", "inactive"):
                    self.assertIn(f"{part}-{state}.xpm", files)
            for glyph in ("close", "hide", "maximize", "maximize-toggled"):
                for look in ("active", "inactive", "prelight", "pressed"):
                    self.assertIn(f"{glyph}-{look}.xpm", files)
            self.assertIn('"  c None"', files["top-left-active.xpm"], "the top corners are not rounded")


if __name__ == "__main__":
    unittest.main()
