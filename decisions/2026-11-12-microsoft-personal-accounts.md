# Microsoft connectors take personal accounts as well as work ones

The owner: the Microsoft connectors allowed only organisations, not personal accounts, and must allow both.

Choice: Alpha Vector's Microsoft client signs in through the `common` authority instead of `organizations`, and Outlook, Calendar and OneDrive name `https://login.microsoftonline.com/common/v2.0` as their issuer. The app registration already accepts personal accounts (Microsoft hands a `consumers` sign-in on to login.live.com rather than refusing it), and every scope the three ask for (Mail.ReadWrite, Mail.Send, Calendars.ReadWrite, Files.ReadWrite, offline_access) works for personal accounts. A personal account answers for Microsoft's consumer tenant, whose issuer is a tenant ID, which the multi-tenant issuer match already accepts.

Check: the catalog test requires a Microsoft client's authority to be `common`, and the issuer test accepts the consumer tenant under `common`.
