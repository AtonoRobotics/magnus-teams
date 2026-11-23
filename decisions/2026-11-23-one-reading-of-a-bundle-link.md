# One reading of a bundle link

A link to an org document the reader may not see is taken out of what they read (K2). The check matched the link's target exactly, so `/people/finance.md#pay`, `?v=1` or a percent-escape kept the link, with the hidden document's name and path. Now one normaliser says which document a link names: it drops the fragment and the query, decodes percent-escapes, and accepts only a path `bundle::place` accepts (`bundle_link::link_path`). Hiding asks it for every link and every reference definition.
