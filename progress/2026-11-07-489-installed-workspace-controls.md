# Installed workspace controls (#489, #475, #477)

The shared controls/Markdown feature from #461 is retained in full. This replacement branch corrects the failures diagnosed from #473 CI and its native-origin and speech-startup review findings.

Local checks:
- Real installed app: composed stream ordering, real uploaded PNG hydration, actual OS clipboard copying, hostile-content sandbox and no-network checks passed.
- Real installed app: sentence-by-sentence appliance speech and immediate Stop passed.
- Native library: 11 tests passed, including child completion, failure, owned cancellation and stop overtaking queued startup.
- Final real-appliance browser shared-controls/Markdown: 2/2 passed, including menu geometry and queued speech startup ownership.
- Full installed message-action test passed: copy, delete, edit/resend, retry, save/export, task creation, task feedback, share and branch.

The KVM-dependent installed workspace research/result/schedule workflow and full suites remain required in CI. This host has no /dev/kvm. Layout, continuity and full baseline acceptance remain tracked by #462–#464; this PR does not complete the user outcome.
