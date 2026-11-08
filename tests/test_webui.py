import os
import re
import unittest

ROOT = os.path.join(os.path.dirname(__file__), "..")
WEB = os.path.join(ROOT, "magnus_teams", "web")

# Words that must never appear as labels in the web UI.
# Listing them here as data for the check is expected.
FORBIDDEN = ("policy", "taint", "boundary")


def read(name):
    with open(os.path.join(WEB, name), encoding="utf-8") as f:
        return f.read()


class WebFilesExist(unittest.TestCase):
    def test_files_exist(self):
        for name in ("index.html", "app.js", "style.css"):
            with self.subTest(name=name):
                path = os.path.join(WEB, name)
                self.assertTrue(os.path.isfile(path), f"missing {name}")
                self.assertGreater(os.path.getsize(path), 0)

    def test_index_links_style_and_script(self):
        html = read("index.html")
        self.assertIn("style.css", html)
        self.assertIn("app.js", html)
        self.assertRegex(html, r'<link[^>]+href="style\.css"')
        self.assertRegex(html, r'<script[^>]+src="app\.js"')

    def test_no_external_resources(self):
        for name in ("index.html", "app.js", "style.css"):
            text = read(name)
            with self.subTest(name=name):
                self.assertNotIn("cdn", text.lower())
                # No external http(s) src/href/url in markup or styles.
                self.assertNotRegex(
                    text,
                    r'(?:src|href)\s*=\s*"https?://',
                )
                self.assertNotRegex(text, r'url\(\s*["\']?https?://')

    def test_app_fetches_workspace(self):
        js = read("app.js")
        self.assertIn("/api/workspace", js)
        self.assertIn("fetch", js)

    def test_app_posts_to_events(self):
        js = read("app.js")
        self.assertIn("/events", js)
        self.assertIn("POST", js)
        self.assertIn("handoff", js)

    def test_app_renders_required_parts(self):
        js = read("app.js")
        for token in ("rooms", "items", "timeline"):
            with self.subTest(token=token):
                self.assertIn(token, js)
        # Work item fields and timeline fields.
        for token in ("owner", "state", "files", "plan", "title"):
            with self.subTest(token=token):
                self.assertIn(token, js)
        for token in ("actor", "type", "issue", "text"):
            with self.subTest(token=token):
                self.assertIn(token, js)

    def test_timeline_newest_first(self):
        js = read("app.js")
        # Sorted or reversed so newest comes first.
        self.assertTrue("sort" in js or "reverse" in js)

    def test_decision_cards_with_plain_buttons(self):
        js = read("app.js")
        self.assertIn("decision", js)
        self.assertIn("options", js)
        for label in ("Allow", "Edit", "Stop"):
            with self.subTest(label=label):
                self.assertIn(label, js)

    def test_no_forbidden_words_in_ui(self):
        for name in ("index.html", "app.js", "style.css"):
            text = read(name).lower()
            for word in FORBIDDEN:
                with self.subTest(name=name, word=word):
                    self.assertNotIn(word, text)

    def test_no_purple_indigo_gradient_and_no_inter(self):
        css = read("style.css")
        low = css.lower()
        self.assertNotIn("purple", low)
        self.assertNotIn("indigo", low)
        self.assertNotIn("linear-gradient", low)
        self.assertNotRegex(css, r'["\']?\bInter\b["\']?\s*[,;]')
        html = read("index.html")
        self.assertNotRegex(html, r'["\']?\bInter\b["\']?\s*[,;]')

    def test_serif_headings_and_sans_body(self):
        css = read("style.css").lower()
        self.assertIn("serif", css)
        self.assertIn("sans-serif", css)

    def test_single_accent_and_reduced_motion(self):
        css = read("style.css")
        self.assertIn("--accent", css)
        self.assertIn("prefers-reduced-motion", css)

    def test_no_emoji_icons(self):
        # Reject pictographic emoji ranges in UI sources.
        emoji = re.compile(
            "[\U0001F300-\U0001FAFF\U00002600-\U000027BF]"
        )
        for name in ("index.html", "app.js", "style.css"):
            with self.subTest(name=name):
                self.assertIsNone(emoji.search(read(name)))

    def test_html_has_landmarks(self):
        html = read("index.html").lower()
        self.assertIn("<header", html)
        self.assertIn("<main", html)
        self.assertIn("rooms", html)
        self.assertIn("work", html)
        self.assertIn("recent activity", html)


if __name__ == "__main__":
    unittest.main()
