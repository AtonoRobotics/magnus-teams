# Every person ID maps to a user directory (2026-11-06)

Person IDs with `@` and other path-breaking characters now map to a `/users/` directory (`sam@example.com` keeps documents under `/users/sam-40example.com/`), so every accepted ID keeps relationship memory and user documents. Frontmatter still names the raw ID; plain IDs map to themselves.
