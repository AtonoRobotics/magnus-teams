# Sign-in codes come out of every mail app's reads, in every form

**Date:** 2026-11-23

## Decision
Codes and sign-in links are taken out of every mail app's results, not only
those of apps with `filter_login_codes` set. A mail app is one in the
catalog's email category (as before), or one whose name, title or address,
or the tool's own name, says mail, inbox, imap or outlook, so an app the
organisation added by its address is covered too. `filter_login_codes`
still turns the filter on for any other app, and a new
`keep_login_codes = true` is the per-app opt-out (it is left out of the
stored connector when false, so the event body is unchanged).

The redaction now also catches:
- digits in equal groups of two to four ("123 456", "123-456", "12 34 56");
- a code on the next line with text after the line naming it ("Your code:",
  a blank line, the code);
- codes of 11 to 64 letters and digits with at least two digits;
- a link carrying a token of 16 or more characters, on a line that speaks
  of signing in or a code, or the line after, though it names no sign-in
  word (Notion's and Slack's magic links).

Code words now match whole words (or their plural), so "barcode" and
"shipping" no longer make a line's numbers codes; the next-line window
would otherwise have spread those false matches.

## Reason
The filter ran only where the flag was set, which an app added by its
address never had, and it missed common formats: grouped digits, a code on
its own line, longer codes and sign-in links that carry only a token. Each
is a live credential an agent could be told to use.
