# The images tools' app name is reserved

`tools::builtin_apps()`, the list of app names no added connector may take,
chained six of the seven built-in tables by hand and left out the images
tools (`image_generate`, `image_edit`, app `images`), which `builtin()` did
include. An admin could add a connector named `images`, whose switches and
policy facts would then be shared with the server's own image tools.

`builtin_apps()` now includes the images table, and a test
(`every_builtin_app_is_reserved`) fails if any built-in's app is missing from
it. The hand-kept chains themselves go in the next change (tool families).
