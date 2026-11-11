"""Where windows go on the computer's desktop (spec rev 145, "Calm for the
agent too"): a new window opens maximized, two windows are snapped to half
the screen each, never a random size or place, so a screenshot shows what
the agent just did.

The rule, applied whenever the window list changes:
  - one window: maximized;
  - two windows: the older on the left half, the newer on the right;
  - three or more: each new window maximized, the others left where they are.
Dialogs and other windows that belong to another window are left to the
window manager, which centres them.

`layout` is the rule alone, for tests; `main` watches the X server.
"""
import sys


def layout(known, current):
    """What to do with each window, as [(window, "maximize" | "left" | "right")].

    `known` is the windows seen before, in the order they appeared; `current`
    the normal windows there now, in the window manager's order (oldest
    first). Returns the actions and the new `known`.
    """
    still = [w for w in known if w in current]
    new = [w for w in current if w not in still]
    order = still + new
    if len(order) == 1:
        return [(order[0], "maximize")], order
    if len(order) == 2:
        if not new and len(known) == 2:
            return [], order
        return [(order[0], "left"), (order[1], "right")], order
    return [(w, "maximize") for w in new], order


def half(side, workarea, extents):
    """A window's client geometry for one half of the work area."""
    x, y, w, h = workarea
    left, right, top, bottom = extents
    width = w // 2
    fx = x if side == "left" else x + width
    return fx, y, width - left - right, h - top - bottom


def main():
    from Xlib import X, display
    from Xlib.protocol import event

    d = display.Display()
    root = d.screen().root
    atom = lambda name: d.intern_atom(name)
    CLIENT_LIST, STATE, MAX_V, MAX_H = atom("_NET_CLIENT_LIST"), atom("_NET_WM_STATE"), atom("_NET_WM_STATE_MAXIMIZED_VERT"), atom("_NET_WM_STATE_MAXIMIZED_HORZ")
    TYPE, NORMAL, WORKAREA, EXTENTS, MOVERESIZE = atom("_NET_WM_WINDOW_TYPE"), atom("_NET_WM_WINDOW_TYPE_NORMAL"), atom("_NET_WORKAREA"), atom("_NET_FRAME_EXTENTS"), atom("_NET_MOVERESIZE_WINDOW")
    root.change_attributes(event_mask=X.PropertyChangeMask)

    def prop(win, name, kind=X.AnyPropertyType):
        try:
            p = win.get_full_property(name, kind)
            return list(p.value) if p else []
        except Exception:
            return []

    def normal(wid):
        win = d.create_resource_object("window", wid)
        try:
            if win.get_wm_transient_for():
                return False
        except Exception:
            return False
        types = prop(win, TYPE)
        return not types or types[0] == NORMAL

    def send(win, kind, data):
        ev = event.ClientMessage(window=win, client_type=kind, data=(32, (data + [0] * 5)[:5]))
        root.send_event(ev, event_mask=X.SubstructureRedirectMask | X.SubstructureNotifyMask)

    def maximized(wid, on):
        send(d.create_resource_object("window", wid), STATE, [1 if on else 0, MAX_V, MAX_H, 1])

    def snap(wid, side):
        win = d.create_resource_object("window", wid)
        maximized(wid, False)
        d.sync()
        area = prop(root, WORKAREA)[:4] or [0, 0, d.screen().width_in_pixels, d.screen().height_in_pixels]
        ext = prop(win, EXTENTS)[:4] or [0, 0, 0, 0]
        x, y, w, h = half(side, area, ext)
        # Gravity north-west, from the pager, with x, y, width and height set.
        send(win, MOVERESIZE, [(2 << 12) | (0xF << 8) | 1, x, y, w, h])

    known = []
    while True:
        current = [w for w in prop(root, CLIENT_LIST) if normal(w)]
        actions, known = layout(known, current)
        for wid, act in actions:
            if act == "maximize":
                maximized(wid, True)
            else:
                snap(wid, act)
        d.flush()
        while True:
            e = d.next_event()
            if e.type == X.PropertyNotify and e.atom == CLIENT_LIST:
                break


if __name__ == "__main__":
    sys.exit(main())
