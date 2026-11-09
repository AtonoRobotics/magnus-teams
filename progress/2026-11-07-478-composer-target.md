# Composer flow target (2026-11-07, #478)

- #473 run 42429152285: 707/709 passed; both desktop_flow variants failed because getByLabel(Message) matched seven named action/menu/input elements.
- Scoped five existing flow/library input locators to exact Message textbox; no product code or assertions removed. Syntax checks pass; checking selector behavior in a browser fixture, then real flows in CI. Local full sandbox flow cannot run without KVM.
