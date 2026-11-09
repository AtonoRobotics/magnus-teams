Console tests now load a page again when Chromium answered a network change with its error page, rather than failing the run (#218). Any other failed load names the request's own net error.
