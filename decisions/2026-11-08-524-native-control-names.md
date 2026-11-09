# Name the message operation and the downloaded document

Native Windows acceptance of the published workspace exposed two UX-03 gaps: the composer named its action “Send”, and the full file view named its primary download action “Download”. The specification requires “Send message” and “Download {filename}”.

Give those existing controls independent accessible names. Keep the composer tooltip and screen-reader text consistent. Use the file's escaped authoritative name, including when its visible title is truncated. The existing download buttons in file chips and task rows already receive filename-specific names; the primary file-view action needs the same context.

The buttons keep their existing handlers, object references and native bridge operations. This is one related control-naming correction discovered during the real Windows workspace acceptance, tracked separately from the remaining host verification in #514.
