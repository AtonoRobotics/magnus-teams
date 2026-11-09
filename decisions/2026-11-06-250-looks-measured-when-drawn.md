# The looks are measured once drawn (#250)

- `desktop/tests/looks.mjs` measures each look again, every 250 ms, until every part it checks is drawn, for up to its 60 s patience. It no longer measures once after a fixed 800 ms pause. Reason: on run 42169676436, at load ~150, a live update was redrawing the task panel when light-solid was measured. Every part of the panel came back missing, and the screenshot shows the panel's column blank. A part still missing at the deadline is reported as before ("… is not drawn"), and the references and the checks are unchanged.
