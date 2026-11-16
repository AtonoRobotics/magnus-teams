"""The Magnus desktop theme, made at image build time (spec rev 145, "The
desktop"): a GTK 3 theme and an xfwm4 window theme in light and dark, the
wallpaper in light and dark, the menu's star, and the variables
`magnus-desktop theme` applies. The colors are the Magnus app's own
(desktop/ui/theme.js, light and dark, blue accent); the wallpaper is the
Wallpaper artboard (docs/design/team-and-computer/Wallpaper.dc.html).

    python3 make_theme.py OUT     writes OUT/themes, OUT/magnus

Nothing here is copied from another theme: the GTK theme is Adwaita, which
GTK itself carries, with Magnus's colors over it, and the window theme's
pictures are drawn here.
"""
import os
import sys

# The app's tokens (desktop/ui/theme.js LIGHT and DARK, the blue accent).
PALETTES = {
    "light": {
        "bar": "#F8F9FB", "line": "#E5E7EB", "win": "#FFFFFF", "title": "#F8F9FB", "bg": "#F6F7F9",
        "fg": "#111318", "fg2": "#4B5563", "muted": "#565E6B", "seg": "#E9ECF2", "raised": "#EEF1F6",
        "accent": "#3B5BDB", "accent_fg": "#FFFFFF", "accent_text": "#364FC7", "accent_soft": "#EDF2FF",
        "term_bg": "#FFFFFF", "term_fg": "#111318", "close_hover": "#FEE2E2",
        # black red green yellow blue magenta cyan white, then the bright ones
        "ansi": ["#111318", "#B91C1C", "#146C34", "#9A4A00", "#3B5BDB", "#6D28D9", "#0F766E", "#565E6B",
                 "#4B5563", "#B91C1C", "#16A34A", "#D97706", "#364FC7", "#4338CA", "#0F766E", "#6B7280"],
    },
    "dark": {
        "bar": "#111318", "line": "#222631", "win": "#111318", "title": "#111318", "bg": "#090A0D",
        "fg": "#ECEFF4", "fg2": "#A6B0C3", "muted": "#8892A6", "seg": "#171A21", "raised": "#1E222B",
        "accent": "#7C8CFF", "accent_fg": "#090A0D", "accent_text": "#9BA7FF", "accent_soft": "#1C2136",
        "term_bg": "#090A0D", "term_fg": "#A6B0C3", "close_hover": "#2D1619",
        "ansi": ["#1E222B", "#F87171", "#34D399", "#FBBF24", "#7C8CFF", "#C4B5FD", "#5EEAD4", "#A6B0C3",
                 "#323846", "#FCA5A5", "#34D399", "#FDBA74", "#9BA7FF", "#A5B4FC", "#5EEAD4", "#ECEFF4"],
    },
}

# Magnus Obsidian (dark) and Light wallpaper palettes, using desktop/ui/theme.js tokens.
WALLPAPER = {
    "dark": {
        "bg": "#090A0D",
        "glow": "#111318",
        "line": "#222631",
        "lineFaint": "#1B1E27",
        "dot": "#323846",
        "accent": "#7C8CFF",
        "accentGlow": "#1C2136",
        "iconFill": "#9BA7FF",
        "mark": "#8892A6",
    },
    "light": {
        "bg": "#F6F7F9",
        "glow": "#FFFFFF",
        "line": "#E5E7EB",
        "lineFaint": "#EDF0F4",
        "dot": "#CBD5E1",
        "accent": "#3B5BDB",
        "accentGlow": "#EDF2FF",
        "iconFill": "#3B5BDB",
        "mark": "#565E6B",
    },
}

# Window frame, in pixels: the artboard's 38-pixel title bar and 12-pixel
# corners, a one-pixel edge.
TITLE_HEIGHT_PX = 38
CORNER_RADIUS_PX = 12
BOTTOM_CORNER_PX = 8
BUTTON_WIDTH_PX = 30
BUTTON_FACE_PX = 26
BUTTON_FACE_WIDTH_PX = 28


def magnus_icon_polygons(color):
    """The Magnus 3-bar forward triad icon in a 64x64 coordinate space (center ~32, 34.5)."""
    return (
        f'<polygon points="18.7,23 24.7,23 16.3,50 10.3,50" fill="{color}" fill-opacity="0.55"/>'
        f'<polygon points="34.4,19 40.4,19 30.8,50 24.8,50" fill="{color}"/>'
        f'<polygon points="47.7,23 53.7,23 45.3,50 39.3,50" fill="{color}" fill-opacity="0.55"/>'
    )


def wallpaper_svg(mode):
    c = WALLPAPER[mode]
    rings = "".join(f'<circle cx="900" cy="400" r="{r}" fill="none" stroke="{c[k]}" stroke-width="1.5"/>'
                    for r, k in [(230, "line"), (340, "line"), (460, "line"), (590, "lineFaint")])
    dots = "".join(f'<circle cx="{x}" cy="{y}" r="{r}" fill="{c[k]}"/>'
                   for x, y, r, k in [(684, 321, 5, "dot"), (1116, 479, 4, "dot"), (784, 81, 4, "dot"), (870, 739, 6, "accent"), (468, 557, 5, "dot"), (1130, 2, 4, "dot")])
    medallion = (
        f'<circle cx="900" cy="400" r="132" fill="{c["glow"]}" stroke="{c["line"]}" stroke-width="1.5"/>'
        f'<circle cx="900" cy="400" r="130" fill="{c["accentGlow"]}" fill-opacity="0.35"/>'
    )
    # Center the 64x64 Magnus icon (centroid 32, 34.5) at (900, 400) scaled 3.6x.
    emblem = f'<g transform="translate(784.8 275.8) scale(3.6)">{magnus_icon_polygons(c["iconFill"])}</g>'
    # Bottom-left mark: 24px Magnus icon and "Magnus" wordmark.
    mark = (
        f'<g transform="translate(56 726) scale(0.375)">{magnus_icon_polygons(c["mark"])}</g>'
        f'<text x="86" y="746" fill="{c["mark"]}" font-family="Inter" font-size="20" font-weight="600" letter-spacing="0.4">Magnus</text>'
    )
    return ('<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="800" viewBox="0 0 1280 800">'
            f'<rect width="1280" height="800" fill="{c["bg"]}"/>{rings}{dots}{medallion}{emblem}{mark}</svg>\n')


def star_svg(color):
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 64 64" fill="none">'
        f'{magnus_icon_polygons(color)}'
        '</svg>\n'
    )


def gtk_css(mode):
    p = PALETTES[mode]
    base = "gtk-contained-dark.css" if mode == "dark" else "gtk-contained.css"
    return f"""/* Magnus {mode}: GTK's own Adwaita with the Magnus app's colors. */
@import url("resource:///org/gtk/libgtk/theme/Adwaita/{base}");

@define-color theme_bg_color {p['win']};
@define-color theme_fg_color {p['fg']};
@define-color theme_base_color {p['win']};
@define-color theme_text_color {p['fg']};
@define-color theme_selected_bg_color {p['accent']};
@define-color theme_selected_fg_color {p['accent_fg']};
@define-color borders {p['line']};

* {{ -gtk-icon-shadow: none; }}
window, .background, dialog {{ background-color: {p['win']}; color: {p['fg']}; }}
.view, iconview, treeview.view, textview text {{ background-color: {p['win']}; color: {p['fg']}; }}
.view:selected, iconview:selected, treeview.view:selected, *:selected, selection {{ background-color: {p['accent']}; color: {p['accent_fg']}; }}
.sidebar, placessidebar, placessidebar list, .thunar .sidebar {{ background-color: {p['bar']}; }}
placessidebar row:selected {{ background-color: {p['accent_soft']}; color: {p['accent_text']}; }}
headerbar, .titlebar, toolbar, .toolbar, menubar {{ background-color: {p['title']}; background-image: none; border-color: {p['line']}; box-shadow: none; color: {p['fg']}; }}
entry {{ background-color: {p['raised']}; color: {p['fg']}; border-color: {p['line']}; border-radius: 7px; box-shadow: none; }}
entry:focus {{ border-color: {p['accent']}; box-shadow: inset 0 0 0 1px {p['accent']}; }}
button {{ border-radius: 7px; background-image: none; background-color: {p['raised']}; border-color: {p['line']}; box-shadow: none; text-shadow: none; color: {p['fg']}; }}
button:hover {{ background-color: {p['seg']}; }}
button.suggested-action {{ background-color: {p['accent']}; color: {p['accent_fg']}; border-color: {p['accent']}; }}
menu, .menu, .context-menu, popover, popover.background {{ background-color: {p['win']}; color: {p['fg']}; border: 1px solid {p['line']}; border-radius: 0; box-shadow: none; }}
menu menuitem {{ padding: 6px 12px; min-height: 18px; }}
menu menuitem:hover {{ background-color: {p['accent_soft']}; color: {p['accent_text']}; }}
separator {{ background-color: {p['line']}; }}
scrollbar {{ background-color: transparent; border: none; }}
scrollbar slider {{ background-color: {p['muted']}; border-radius: 8px; min-width: 6px; min-height: 6px; }}
tooltip, tooltip.background {{ background-color: {p['raised']}; color: {p['fg']}; border: 1px solid {p['line']}; border-radius: 6px; }}
vte-terminal {{ padding: 16px 18px; }}

/* The computer's apps run as root by design (the computer is the sandbox),
   so Files does not warn about it on every window. */
.thunar infobar.warning, .thunar infobar.warning > revealer > box {{ min-height: 0; margin: 0; padding: 0; border: none; background: transparent; }}
.thunar infobar.warning label {{ font-size: 1px; color: transparent; min-height: 0; margin: 0; padding: 0; }}

/* The top bar: the Magnus menu, the pinned apps, the open windows, the clock. */
.xfce4-panel.background, #XfcePanelWindow {{ background-color: {p['bar']}; background-image: none; color: {p['fg']}; box-shadow: inset 0 -1px {p['line']}; border: none; }}
.xfce4-panel button, #XfcePanelWindow button {{ border: none; border-radius: 7px; background-color: transparent; background-image: none; box-shadow: none; color: {p['fg']}; padding: 0 10px; margin: 4px 2px; min-height: 26px; }}
.xfce4-panel button:hover, #XfcePanelWindow button:hover {{ background-color: {p['seg']}; }}
.xfce4-panel button:checked, #XfcePanelWindow button:checked {{ background-color: {p['seg']}; color: {p['fg']}; }}
#tasklist-button {{ font-size: 9pt; }}
#tasklist-button:checked {{ background-color: {p['seg']}; }}
#launcher-button {{ padding: 0 6px; }}
#applicationmenu-button label, #applicationsmenu-button label {{ font-weight: 600; }}
.xfce4-panel label, #XfcePanelWindow label {{ color: {p['fg']}; }}
#clock-button label, .xfce4-panel .clock label {{ color: {p['muted']}; font-weight: 500; }}
.xfce4-panel separator, #XfcePanelWindow separator {{ background-color: {p['line']}; min-width: 1px; margin: 9px 6px; }}
#separator-2, #separator-6 {{ color: {p['line']}; }}
window vte-terminal, vte-terminal {{ padding: 12px 14px; }}
"""


def gtk2_rc(mode):
    p = PALETTES[mode]
    return f'gtk-color-scheme = "bg_color:{p["win"]}\\nfg_color:{p["fg"]}\\nbase_color:{p["win"]}\\ntext_color:{p["fg"]}\\nselected_bg_color:{p["accent"]}\\nselected_fg_color:{p["accent_fg"]}"\n'


def index_theme(mode):
    return f"[Desktop Entry]\nType=X-GNOME-Metatheme\nName=Magnus-{mode.title()}\nComment=The Magnus desktop, {mode}\n\n[X-GNOME-Metatheme]\nGtkTheme=Magnus-{mode.title()}\nMetacityTheme=Magnus-{mode.title()}\nIconTheme=Papirus-{mode.title()}\n"


def xpm(rows, colors):
    """An XPM image from rows of one-character codes and {code: color}."""
    w, h = len(rows[0]), len(rows)
    lines = [f'"{w} {h} {len(colors)} 1",'] + [f'"{k} c {v}",' for k, v in colors.items()] + [f'"{r}",' for r in rows]
    lines[-1] = lines[-1].rstrip(",")
    return "/* XPM */\nstatic char * image[] = {\n" + "\n".join(lines) + "\n};\n"


def inside_corner(x, y, r):
    """Whether pixel (x, y) of an r-by-r top-left corner is inside the rounding."""
    cx, cy = r - 0.5, r - 0.5
    px, py = x + 0.5, y + 0.5
    if px >= r - 0.5 or py >= r - 0.5:
        return True
    return (px - cx) ** 2 + (py - cy) ** 2 <= (r - 0.5) ** 2


def on_corner_edge(x, y, r):
    """Inside the rounding, with a neighbour outside it or off the image."""
    if not inside_corner(x, y, r):
        return False
    return any(nx < 0 or ny < 0 or not inside_corner(nx, ny, r) for nx, ny in [(x - 1, y), (x, y - 1)])


def top_left(r, h, fill, edge):
    rows = []
    for y in range(h):
        row = ""
        for x in range(r):
            if y < r and not inside_corner(x, y, r):
                row += " "
            elif (y < r and on_corner_edge(x, y, r)) or x == 0:
                row += "e"
            else:
                row += "f"
        rows.append(row)
    return rows


def bottom_left(r, fill, edge):
    rows = [row for row in reversed(top_left(r, r, fill, edge))]
    rows[-1] = "".join("e" if c == "f" else c for c in rows[-1])
    return rows


def mirror(rows):
    return [row[::-1] for row in rows]


GLYPHS = {
    # 12-by-12 marks, drawn as the artboard's buttons: minimize, maximize, close.
    "hide": ["            "] * 6 + [" xxxxxxxxxx "] + ["            "] * 5,
    "maximize": ["            ", " xxxxxxxxxx ", " x        x ", " x        x ", " x        x ", " x        x ",
                 " x        x ", " x        x ", " x        x ", " x        x ", " xxxxxxxxxx ", "            "],
    "maximize-toggled": ["            ", "   xxxxxxxx ", "   x      x ", " xxxxxxxx x ", " x      x x ", " x      x x ",
                         " x      x x ", " x      xxx ", " x      x   ", " xxxxxxxx   ", "            ", "            "],
    "close": ["            ", " x        x ", "  x      x  ", "   x    x   ", "    x  x    ", "     xx     ",
              "     xx     ", "    x  x    ", "   x    x   ", "  x      x  ", " x        x ", "            "],
}


def button(glyph, face, ink, background, edge):
    """A button the title bar's height: a rounded face behind the glyph."""
    w, h = BUTTON_WIDTH_PX, TITLE_HEIGHT_PX
    fx0, fy0 = (w - BUTTON_FACE_WIDTH_PX) // 2, (h - BUTTON_FACE_PX) // 2
    gx0, gy0 = (w - 12) // 2, (h - 12) // 2
    rows = []
    for y in range(h):
        row = ""
        for x in range(w):
            g = GLYPHS[glyph]
            if gx0 <= x < gx0 + 12 and gy0 <= y < gy0 + 12 and g[y - gy0][x - gx0] == "x":
                row += "i"
                continue
            fx, fy = x - fx0, y - fy0
            in_face = face and 0 <= fx < BUTTON_FACE_WIDTH_PX and 0 <= fy < BUTTON_FACE_PX
            if in_face:
                rr = 6
                cx = min(fx, BUTTON_FACE_WIDTH_PX - 1 - fx)
                cy = min(fy, BUTTON_FACE_PX - 1 - fy)
                in_face = not (cx < rr and cy < rr and not inside_corner(cx, cy, rr))
            row += "h" if in_face else "f"
        rows.append(row)
    # The title bar's top and bottom edges run through the buttons too.
    rows[0] = rows[-1] = "e" * w
    colors = {"f": background, "i": ink, "e": edge}
    if face:
        colors["h"] = face
    return rows, colors


def xfwm4_files(mode):
    p = PALETTES[mode]
    files = {
        "themerc": "\n".join([
            f"active_text_color={p['fg']}", f"inactive_text_color={p['muted']}",
            "title_horizontal_offset=0", "title_vertical_offset_active=0", "title_vertical_offset_inactive=0",
            "button_offset=6", "button_spacing=2", "title_shadow_active=false", "title_shadow_inactive=false",
            "full_width_title=true", "maximized_offset=0", "shadow_delta_height=0", "shadow_delta_width=0",
            "shadow_delta_x=0", "shadow_delta_y=0", "shadow_opacity=0", "show_app_icon=false", "",
        ])
    }
    r = CORNER_RADIUS_PX
    for state, title_bg in [("active", p["title"]), ("inactive", p["title"])]:
        colors = {" ": "None", "e": p["line"], "f": title_bg}
        tl = top_left(r, TITLE_HEIGHT_PX, title_bg, p["line"])
        tl[-1] = tl[-1].replace("f", "e")
        files[f"top-left-{state}.xpm"] = xpm(tl, colors)
        files[f"top-right-{state}.xpm"] = xpm(mirror(tl), colors)
        title = ["e"] + ["f"] * (TITLE_HEIGHT_PX - 2) + ["e"]
        for i in range(1, 6):
            files[f"title-{i}-{state}.xpm"] = xpm(title, colors)
        files[f"left-{state}.xpm"] = xpm(["e"], {"e": p["line"]})
        files[f"right-{state}.xpm"] = xpm(["e"], {"e": p["line"]})
        files[f"bottom-{state}.xpm"] = xpm(["e"], {"e": p["line"]})
        bl_colors = {" ": "None", "e": p["line"], "f": p["win"]}
        bl = bottom_left(BOTTOM_CORNER_PX, p["win"], p["line"])
        files[f"bottom-left-{state}.xpm"] = xpm(bl, bl_colors)
        files[f"bottom-right-{state}.xpm"] = xpm(mirror(bl), bl_colors)
        for glyph in GLYPHS:
            if state == "active":
                looks = [("active", None, p["muted"]), ("prelight", p["seg"], p["fg"]), ("pressed", p["line"], p["fg"])]
            else:
                looks = [("inactive", None, p["line"])]
            for look, face, ink in looks:
                if glyph == "close" and face:
                    face = p["close_hover"]
                rows, colors = button(glyph, face, ink, title_bg, p["line"])
                files[f"{glyph}-{look}.xpm"] = xpm(rows, colors)
    return files


def term_vars(mode):
    p = PALETTES[mode]
    return {
        "GTK_THEME_NAME": f"Magnus-{mode.title()}",
        "WM_THEME_NAME": f"Magnus-{mode.title()}",
        "ICON_THEME_NAME": f"Papirus-{mode.title()}",
        "WALLPAPER": f"/usr/share/backgrounds/magnus/magnus-{mode}.png",
        "TERM_FG": p["term_fg"], "TERM_BG": p["term_bg"], "TERM_CURSOR": p["accent_text"],
        "TERM_PALETTE": ";".join(p["ansi"]),
    }


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)


def main(out):
    for mode in ("light", "dark"):
        name = f"Magnus-{mode.title()}"
        write(f"{out}/themes/{name}/index.theme", index_theme(mode))
        write(f"{out}/themes/{name}/gtk-3.0/gtk.css", gtk_css(mode))
        write(f"{out}/themes/{name}/gtk-2.0/gtkrc", gtk2_rc(mode))
        for fname, body in xfwm4_files(mode).items():
            write(f"{out}/themes/{name}/xfwm4/{fname}", body)
        write(f"{out}/magnus/wallpaper-{mode}.svg", wallpaper_svg(mode))
        write(f"{out}/magnus/{mode}.vars", "".join(f"{k}='{v}'\n" for k, v in term_vars(mode).items()))
    write(f"{out}/magnus/star.svg", star_svg(PALETTES["light"]["accent"]))


if __name__ == "__main__":
    main(sys.argv[1])
